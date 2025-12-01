use std::io::Cursor;
use std::sync::{Arc};
use mp4_atom::{ Any, ReadFrom };
use bytes::Bytes;


#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum SegmentParseError {
    #[error("Invalid payload: {msg}")]
    InvalidPayload{ msg: String },

    #[error("Cursor error: {msg}")]
    CursorError{ msg: String },

    #[error("IO error: {msg}")]
    IoError{ msg: String },
}

impl From<anyhow::Error> for SegmentParseError {
    fn from(err: anyhow::Error) -> Self {
        SegmentParseError::InvalidPayload {
            msg: err.to_string(),
        }
    }
}

#[derive(uniffi::Record)]
pub struct ParsedSegment {
    pub video_frames: Vec<Arc<DemuxedFrame>>,
    pub audio_frames: Vec<Arc<DemuxedFrame>>,
}

#[derive(Debug, uniffi::Object)]
pub struct DemuxedFrame {
    pub data: Vec<u8>,
    pub timestamp: Option<u64>,
    pub duration: Option<u32>,
    pub is_keyframe: bool,
}

// Wrap internal state in Mutex for interior mutability
#[derive(uniffi::Object)]
pub struct SegmentParser {
    pub hevc: bool, // true for H.265, false for H.264
}

#[uniffi::export]
impl SegmentParser {
    #[uniffi::constructor]
    pub fn new(hevc: bool) -> Arc<Self>{
        Self { hevc }.into()
    }

    /// Parse fMP4 segment and extract raw video/audio frames
    pub fn parse_segment(&self, payload: Vec<u8>) -> anyhow::Result<ParsedSegment, SegmentParseError> {
        let mut cursor = Cursor::new(payload);
        let mut video_frames = Vec::new();
        let mut audio_frames = Vec::new();
        let mut current_moof: Option<mp4_atom::Moof> = None;

        while let Ok(atom) = Any::read_from(&mut cursor) {
            match atom {
                // Movie Fragment Box
                Any::Moof(m) => {
                    current_moof = Some(m);
                }
                // Media Data Box
                Any::Mdat(m) => {
                    if current_moof.is_none() {
                        continue; // Skip mdat without preceding moof
                    }
                    let moof = current_moof.take().unwrap();
                    self.extract_frames_from_mdat(
                        &m.data,
                        &moof,
                        &mut video_frames,
                        &mut audio_frames
                    )?;
                }

                // Skip other boxes
                _ => {}
            }
        }

        Ok(ParsedSegment { video_frames, audio_frames })
    }



    /// Convert length-prefixed NALUs to Annex-B format (0x00000001 prefix)
    fn extract_video_nalus(&self, sample: &[u8]) -> anyhow::Result<Vec<u8>, SegmentParseError> {
        let mut result = Vec::new();
        let mut offset = 0;

        while offset + 4 <= sample.len() {
            // Read NALU length (big-endian u32)
            let nal_size = u32::from_be_bytes([
                sample[offset],
                sample[offset + 1],
                sample[offset + 2],
                sample[offset + 3],
            ]) as usize;
            offset += 4;

            if offset + nal_size > sample.len() {
                break; // Truncated NALU
            }

            // Add Annex-B start code (0x00000001)
            result.extend_from_slice(&[0x00, 0x00, 0x00, 0x01]);

            // Add NALU data
            result.extend_from_slice(&sample[offset..offset + nal_size]);

            offset += nal_size;
        }

        Ok(result)
    }

    /// Extract raw AAC frame (remove any container headers if present)
    fn extract_aac_frame(&self, sample: &[u8]) -> anyhow::Result<Vec<u8>, SegmentParseError> {
        // For AAC in MP4, the sample data is usually already raw AAC
        // But you might need to add ADTS header if required by your decoder
        Ok(sample.to_vec())
    }

    /// Detect if sample contains video data (heuristic based on NALU patterns)
    fn is_video_sample(&self, sample: &[u8]) -> bool {
        if sample.len() < 8 {
            return false;
        }

        // Check if it starts with a reasonable NALU length
        let nal_size = u32::from_be_bytes([sample[0], sample[1], sample[2], sample[3]]) as usize;

        // NALU size should be reasonable and within sample bounds
        if nal_size == 0 || nal_size > sample.len() - 4 {
            return false;
        }

        // Check NALU header patterns
        if sample.len() > 4 {
            match self.hevc {
                false => {
                    // H.264: check forbidden_zero_bit and nal_unit_type
                    let nal_header = sample[4];
                    let forbidden_bit = (nal_header >> 7) & 1;
                    let nal_type = nal_header & 0x1f;
                    forbidden_bit == 0 && nal_type <= 24
                }
                true => {
                    // H.265: check forbidden_zero_bit
                    if sample.len() > 5 {
                        let nal_header = u16::from_be_bytes([sample[4], sample[5]]);
                        let forbidden_bit = (nal_header >> 15) & 1;
                        forbidden_bit == 0
                    } else {
                        false
                    }
                }
            }
        } else {
            false
        }
    }

    /// Detect whether a video sample is a keyframe (reusing your existing logic)
    pub fn is_keyframe_sample(&self, sample: &[u8]) -> bool {
        let mut offset = 0;
        while offset + 4 <= sample.len() {
            let nal_size = u32::from_be_bytes([
                sample[offset],
                sample[offset + 1],
                sample[offset + 2],
                sample[offset + 3],
            ]) as usize;
            offset += 4;

            if offset + nal_size > sample.len() {
                break;
            }

            match self.hevc {
                false => {
                    let nal_type = sample[offset] & 0x1f;
                    match nal_type {
                        5 => {
                            return true;
                        } // IDR slice
                        1 => {
                            return false;
                        } // non-IDR slice
                        _ => {} // skip SPS/PPS/SEI
                    }
                }
                true => {
                    if nal_size >= 2 {
                        let nal_type =
                            (u16::from_be_bytes([sample[offset], sample[offset + 1]]) >> 9) & 0x3f;
                        if nal_type == 19 || nal_type == 20 || nal_type == 21 {
                            return true; // IDR_W_RADL, IDR_N_LP, CRA
                        }
                    }
                }
            }

            offset += nal_size;
        }

        false
    }
}

impl SegmentParser {
    fn extract_frames_from_mdat(
        &self,
        mdat_data: &[u8],
        moof: &mp4_atom::Moof,
        video_frames: &mut Vec<Arc<DemuxedFrame>>,
        audio_frames: &mut Vec<Arc<DemuxedFrame>>
    ) -> anyhow::Result<(), SegmentParseError> {
        let mut data_offset = 0;

        for traf in &moof.traf {
            let trun = &traf.trun[0]; // For simplicity, only handle first trun
            if trun.entries.is_empty() {
                return Err(SegmentParseError::InvalidPayload {
                    msg : "Invalid trun.entries".to_string()
                });
            }
            let mut sample_offset = if trun.data_offset.is_some() {
                trun.data_offset.unwrap() as usize
            } else {
                data_offset
            };

            for entry in &trun.entries {
                let sample_size = entry.size.unwrap_or(0) as usize;
                let sample_duration = entry.duration;
                let sample_timestamp = entry.cts.map(|offset| offset as u64);

                if sample_offset + sample_size > mdat_data.len() {
                    break;
                }

                let sample_data = &mdat_data[sample_offset..sample_offset + sample_size];

                // Determine if this is video or audio track based on some heuristics
                // You might want to pass track type information from initialization segment
                if self.is_video_sample(sample_data) {
                    let raw_nalus = self.extract_video_nalus(sample_data)?;
                    let is_keyframe = self.is_keyframe_sample(sample_data);

                    video_frames.push(Arc::new(DemuxedFrame {
                        data: raw_nalus,
                        timestamp: sample_timestamp,
                        duration: sample_duration,
                        is_keyframe,
                    }));
                } else {
                    // Assume audio (AAC)
                    let raw_aac = self.extract_aac_frame(sample_data)?;

                    audio_frames.push(Arc::new(DemuxedFrame {
                        data: raw_aac,
                        timestamp: sample_timestamp,
                        duration: sample_duration,
                        is_keyframe: false, // Audio frames don't have keyframes
                    }));
                }

                sample_offset += sample_size;
            }

            data_offset = sample_offset;
        }

        Ok(())
    }

    pub fn extract_frames_from_mdat_enhanced(
        &self,
        mdat_data: &[u8],
        moof: &mp4_atom::Moof,
        video_frames: &mut Vec<DemuxedFrame>,
        audio_frames: &mut Vec<DemuxedFrame>
    ) -> anyhow::Result<(), SegmentParseError> {
        let mut data_offset = 0;

        for traf in &moof.traf {
            let track_id = traf.tfhd.track_id;

            // Determine track type based on your muxer configuration
            // Since you add video stream first, then audio stream in muxer builder
            let is_video_track = track_id == 1;
            let trun = &traf.trun[0]; // For simplicity, only handle first trun
            if trun.entries.is_empty() {
                return Err(SegmentParseError::InvalidPayload {
                    msg : "Invalid trun.entries".to_string()
                });
            }

            let mut sample_offset = data_offset;

            for (sample_index, entry) in trun.entries.iter().enumerate() {
                let sample_size = entry.size.unwrap_or(0) as usize;
                let sample_duration = entry.duration;
                let sample_timestamp = entry.cts.map(|offset| offset as u64);

                if sample_offset + sample_size > mdat_data.len() {
                    return Err(SegmentParseError::InvalidPayload {
                        msg : format!("Sample {} size {} exceeds mdat data length {}",
                                      sample_index,
                                      sample_size,
                                      mdat_data.len()
                        )});
                }

                let sample_data = &mdat_data[sample_offset..sample_offset + sample_size];

                if is_video_track {
                    // For video frames, detect codec type and keyframes
                    let is_keyframe = if sample_data.len() >= 4 {
                        self.is_keyframe_sample(sample_data)
                    } else {
                        false
                    };

                    video_frames.push(DemuxedFrame {
                        data: Bytes::copy_from_slice(sample_data).to_vec(),
                        timestamp: sample_timestamp,
                        duration: sample_duration,
                        is_keyframe,
                    });
                } else {
                    // For audio frames (MP4A/AAC)
                    audio_frames.push(DemuxedFrame {
                        data: Bytes::copy_from_slice(sample_data).to_vec(),
                        timestamp: sample_timestamp,
                        duration: sample_duration,
                        is_keyframe: false,
                    });
                }

                sample_offset += sample_size;
            }

            data_offset = sample_offset;
        }

        Ok(())
    }
}
uniffi::setup_scaffolding!();