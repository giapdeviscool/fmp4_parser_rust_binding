use mp4_atom::{ Any, ReadFrom };
use std::io::Cursor;
use bytes::Bytes;

#[derive(Debug, Clone)]
pub struct DemuxedFrame {
    pub data: Vec<u8>,
    pub timestamp: Option<u32>,  // Changed from u64 to u32
    pub duration: Option<u32>,
    pub is_keyframe: bool,
}

#[derive(Debug)]
pub struct ParsedSegment {
    pub video_frames: Vec<DemuxedFrame>,
    pub audio_frames: Vec<DemuxedFrame>,
}

pub struct SegmentParser {
    pub hevc: bool,
}

impl SegmentParser {
    pub fn new(hevc: bool) -> Self {
        Self { hevc }
    }

    pub fn parse_segment(&self, payload: Bytes) -> anyhow::Result<ParsedSegment> {
        let mut cursor = Cursor::new(payload);
        let mut video_frames = Vec::new();
        let mut audio_frames = Vec::new();
        let mut current_moof: Option<_> = None;

        while let Ok(atom) = Any::read_from(&mut cursor) {
            match atom {
                Any::Moof(m) => {
                    current_moof = Some(m);
                }
                Any::Mdat(m) => {
                    if current_moof.is_none() {
                        continue;
                    }
                    let moof = current_moof.take().unwrap();
                    self.extract_frames_from_mdat_enhanced(
                        &m.data,
                        &moof,
                        &mut video_frames,
                        &mut audio_frames
                    )?;
                }
                _ => {}
            }
        }

        Ok(ParsedSegment {
            video_frames,
            audio_frames,
        })
    }

    pub fn extract_frames_from_mdat_enhanced(
        &self,
        mdat_data: &[u8],
        moof: &mp4_atom::Moof,
        video_frames: &mut Vec<DemuxedFrame>,
        audio_frames: &mut Vec<DemuxedFrame>
    ) -> anyhow::Result<()> {
        let mut data_offset = 0;

        for traf in &moof.traf {
            let track_id = traf.tfhd.track_id;
            let is_video_track = track_id == 1;
            let trun = &traf.trun[0];

            if trun.entries.is_empty() {
                return Err(anyhow::anyhow!("No entries in TRUN"));
            }

            let default_sample_size = traf.tfhd.default_sample_size.unwrap_or(0);
            let mut sample_offset = data_offset;

            let base_time = traf.tfdt.as_ref().map(|tfdt| tfdt.base_media_decode_time).unwrap_or(0);
            let mut accumulated_time = base_time;

            for (sample_index, entry) in trun.entries.iter().enumerate() {
                let sample_size = entry.size.unwrap_or(default_sample_size) as usize;
                let sample_duration = entry.duration;

                // Convert u64 timestamp to u32 (handle overflow by taking lower 32 bits or clamping)
                let sample_timestamp = if let Some(cts) = entry.cts {
                    Some((accumulated_time + cts as u64) as u32)
                } else {
                    Some(accumulated_time as u32)
                };

                if sample_offset + sample_size > mdat_data.len() {
                    return Err(
                        anyhow::anyhow!(
                            "Sample {} size {} exceeds mdat data length {}",
                            sample_index,
                            sample_size,
                            mdat_data.len()
                        )
                    );
                }

                let sample_data = &mdat_data[sample_offset..sample_offset + sample_size];

                if is_video_track {
                    let is_keyframe = if sample_data.len() >= 5 {
                        self.is_keyframe_sample(sample_data)
                    } else {
                        false
                    };

                    video_frames.push(DemuxedFrame {
                        data: sample_data.to_vec(),
                        timestamp: sample_timestamp,
                        duration: sample_duration,
                        is_keyframe,
                    });
                } else {
                    audio_frames.push(DemuxedFrame {
                        data: sample_data.to_vec(),
                        timestamp: sample_timestamp,
                        duration: sample_duration,
                        is_keyframe: false,
                    });
                }

                sample_offset += sample_size;

                if let Some(duration) = sample_duration {
                    accumulated_time += duration as u64;
                }
            }

            data_offset = sample_offset;
        }

        Ok(())
    }

    pub fn is_keyframe_sample(&self, sample: &[u8]) -> bool {
        let mut offset = 0;
        let mut found_slice = false;

        while offset + 4 <= sample.len() {
            let nal_size = u32::from_be_bytes([
                sample[offset],
                sample[offset + 1],
                sample[offset + 2],
                sample[offset + 3],
            ]) as usize;
            offset += 4;

            if offset + nal_size > sample.len() || nal_size == 0 {
                break;
            }

            match self.hevc {
                false => {
                    let nal_type = sample[offset] & 0x1f;
                    match nal_type {
                        5 => return true,
                        1 => found_slice = true,
                        _ => {}
                    }
                }
                true => {
                    if offset + 1 < sample.len() {
                        let nal_header = u16::from_be_bytes([sample[offset], sample[offset + 1]]);
                        let nal_type = (nal_header >> 9) & 0x3f;

                        match nal_type {
                            19 | 20 | 21 | 16 | 17 | 18 => return true,
                            0..=9 => found_slice = true,
                            _ => {}
                        }
                    }
                }
            }

            offset += nal_size;
        }

        if found_slice {
            return false;
        }

        false
    }
}