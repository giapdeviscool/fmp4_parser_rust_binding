
use anyhow::Result;
use base64::{Engine as _, prelude::BASE64_STANDARD};
use futures::{StreamExt as _, sink::SinkExt};
use iroh::{Watcher,
    Endpoint, EndpointAddr, RelayMap, RelayMode, RelayUrl,
    endpoint::{Builder, ConnectOptions, Connection, ConnectionType, RecvStream, SendStream},
};
use iroh_quinn_proto::{TransportConfig, congestion::BbrConfig};
use std::{str::FromStr as _, sync::Arc, time::Duration};
use tokio::{runtime::Runtime, sync::Mutex};
use tokio_util::{
    bytes::Bytes,
    codec::{FramedRead, FramedWrite, LengthDelimitedCodec},
};

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum ErmisCallError {
    #[error("Connection error: {msg}")]
    ConnectionError { msg: String },
    #[error("Not connected")]
    NotConnected,
    #[error("Stream error: {msg}")]
    StreamError { msg: String },
    #[error("Encoding error: {msg}")]
    EncodingError { msg: String },
    #[error("IO error: {msg}")]
    IoError { msg: String },
}

#[derive(Debug, Clone, uniffi::Enum)]
pub enum ConnectionTypeWrapper {
    Direct,
    Relay,
    Mixed,
    None,
}

impl From<ConnectionType> for ConnectionTypeWrapper {
    fn from(ct: ConnectionType) -> Self {
        match ct {
            ConnectionType::Direct(_) => ConnectionTypeWrapper::Direct,
            ConnectionType::Relay(_) => ConnectionTypeWrapper::Relay,
            ConnectionType::Mixed(_, _) => ConnectionTypeWrapper::Mixed,
            ConnectionType::None => ConnectionTypeWrapper::None,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct ConnectionStats {
    pub connection_type: Option<ConnectionTypeWrapper>,
    pub round_trip_time_ms: Option<u64>,
    pub packet_loss: Option<f64>,
}

struct EndpointState {
    cur_connection: Option<Connection>,
    cur_sender: Option<FramedWrite<SendStream, LengthDelimitedCodec>>,
    cur_receiver: Option<FramedRead<RecvStream, LengthDelimitedCodec>>,
}

#[derive(uniffi::Object)]
pub struct ErmisCallEndpoint {
    iroh_endpoint: Endpoint,
    state: Mutex<EndpointState>,
    runtime: Runtime,
}

#[uniffi::export]
impl ErmisCallEndpoint {
    #[uniffi::constructor]
    pub fn new(relay_urls: Vec<String>) -> Result<Arc<Self>, ErmisCallError> {
        let runtime = Runtime::new().map_err(|e| ErmisCallError::IoError { 
            msg: e.to_string() 
        })?;
        let relay_urls_refs: Vec<&str> = relay_urls.iter().map(|s| s.as_str()).collect();
        
        let res: Result<Endpoint> = runtime.block_on(async move {
            let iroh_endpoint = Builder::empty(RelayMode::Custom(RelayMap::from_iter(
                relay_urls_refs
                    .iter()
                    .map(|url| RelayUrl::from_str(url).unwrap()),
            )))
            .alpns(vec![b"ermis-call".to_vec()])
            .bind()
            .await?;
            iroh_endpoint.online().await;
            println!("{:?}", iroh_endpoint.addr());
            Ok(iroh_endpoint)
        });
        
        Ok(Arc::new(Self {
            iroh_endpoint: res.map_err(|e| ErmisCallError::ConnectionError { 
                msg: e.to_string() 
            })?,
            state: Mutex::new(EndpointState {
                cur_connection: None,
                cur_sender: None,
                cur_receiver: None,
            }),
            runtime,
        }))
    }

    pub fn get_connection_stats(&self) -> ConnectionStats {
        let handle = self.runtime.handle();
        handle.block_on(async {
            let state = self.state.lock().await;
            ConnectionStats {
                connection_type: self.connection_type(&state),
                round_trip_time_ms: self.round_trip_time(&state).map(|d| d.as_millis() as u64),
                packet_loss: self.cur_packet_loss(&state),
            }
        })
    }

    pub fn connect(&self, addr: String) -> Result<(), ErmisCallError> {
        let handle = self.runtime.handle();
        let endpoint = self.iroh_endpoint.clone();
        
        handle.block_on(async {
            let addr_bytes = BASE64_STANDARD.decode(addr).map_err(|e| ErmisCallError::EncodingError { 
                msg: e.to_string() 
            })?;
            let addr: EndpointAddr = bitcode::deserialize(&addr_bytes).map_err(|e| ErmisCallError::EncodingError { 
                msg: e.to_string() 
            })?;
            println!("connecting to {:?}", addr);
            
            let mut transport_config = TransportConfig::default();
            transport_config.congestion_controller_factory(Arc::new(BbrConfig::default()));
            
            let conn = endpoint
                .connect_with_opts(
                    addr,
                    b"ermis-call",
                    ConnectOptions::new().with_transport_config(Arc::new(transport_config)),
                )
                .await
                .map_err(|e| ErmisCallError::ConnectionError { msg: e.to_string() })?
                .await
                .map_err(|e| ErmisCallError::ConnectionError { msg: e.to_string() })?;
            
            println!("{:?}", conn.stable_id());
            
            let mut state = self.state.lock().await;
            state.cur_connection = Some(conn);
            Ok(())
        })
    }

    pub fn get_local_endpoint_addr(&self) -> Result<String, ErmisCallError> {
        let addr_bytes = bitcode::serialize(&self.iroh_endpoint.addr())
            .map_err(|e| ErmisCallError::EncodingError { msg: e.to_string() })?;
        let addr_str = BASE64_STANDARD.encode(addr_bytes);
        Ok(addr_str)
    }

    pub fn accept_connection(&self) -> Result<(), ErmisCallError> {
        let handle = self.runtime.handle();
        let endpoint = self.iroh_endpoint.clone();
        
        handle.block_on(async {
            if let Some(incoming) = endpoint.accept().await {
                let (conn, _za) = incoming.accept().unwrap().into_0rtt().unwrap();
                let mut state = self.state.lock().await;
                state.cur_connection = Some(conn);
                Ok(())
            } else {
                Err(ErmisCallError::ConnectionError { 
                    msg: "Cannot accept connection".to_string() 
                })
            }
        })
    }

    pub fn accept_bidi_stream(&self) -> Result<(), ErmisCallError> {
        let handle = self.runtime.handle();
        
        handle.block_on(async {
            let mut state = self.state.lock().await;
            if let Some(conn) = &state.cur_connection {
                if let Ok((send_stream, recv_stream)) = conn.accept_bi().await {
                    state.cur_sender = Some(FramedWrite::new(send_stream, LengthDelimitedCodec::new()));
                    state.cur_receiver = Some(FramedRead::new(recv_stream, LengthDelimitedCodec::new()));
                    Ok(())
                } else {
                    Err(ErmisCallError::StreamError { 
                        msg: "Failed to accept bidirectional stream".to_string() 
                    })
                }
            } else {
                Err(ErmisCallError::NotConnected)
            }
        })
    }

    pub fn open_bidi_stream(&self) -> Result<(), ErmisCallError> {
        let handle = self.runtime.handle();
        
        handle.block_on(async {
            let mut state = self.state.lock().await;
            if let Some(conn) = &state.cur_connection {
                if let Ok((send_stream, recv_stream)) = conn.open_bi().await {
                    state.cur_sender = Some(FramedWrite::new(send_stream, LengthDelimitedCodec::new()));
                    state.cur_receiver = Some(FramedRead::new(recv_stream, LengthDelimitedCodec::new()));
                    Ok(())
                } else {
                    Err(ErmisCallError::StreamError { 
                        msg: "Failed to open bidirectional stream".to_string() 
                    })
                }
            } else {
                Err(ErmisCallError::NotConnected)
            }
        })
    }

    pub fn send(&self, data: Vec<u8>) -> Result<(), ErmisCallError> {
        let handle = self.runtime.handle();
        
        handle.block_on(async {
            let mut state = self.state.lock().await;
            if let Some(sender) = &mut state.cur_sender {
                sender
                    .send(Bytes::copy_from_slice(&data))
                    .await
                    .map_err(|e| ErmisCallError::StreamError { msg: e.to_string() })?;
                Ok(())
            } else {
                Err(ErmisCallError::NotConnected)
            }
        })
    }

    pub fn recv(&self) -> Result<Vec<u8>, ErmisCallError> {
        let handle = self.runtime.handle();
        
        handle.block_on(async {
            let mut state = self.state.lock().await;
            if let Some(receiver) = &mut state.cur_receiver {
                let bytes = receiver
                    .next()
                    .await
                    .ok_or_else(|| ErmisCallError::ConnectionError { 
                        msg: "Connection closed".to_string() 
                    })?
                    .map_err(|e| ErmisCallError::StreamError { msg: e.to_string() })?;
                Ok(bytes.freeze().to_vec())
            } else {
                Err(ErmisCallError::NotConnected)
            }
        })
    }
}

impl ErmisCallEndpoint {
    fn connection_type(&self, state: &EndpointState) -> Option<ConnectionTypeWrapper> {
        if let Some(conn) = state.cur_connection.as_ref() {
            let c = self.iroh_endpoint.conn_type(conn.remote_id().unwrap());
            return c.map(|mut ct| ct.get().into());
        }
        None
    }

    fn round_trip_time(&self, state: &EndpointState) -> Option<Duration> {
        state.cur_connection.as_ref().map(|conn| conn.rtt())
    }

    fn cur_packet_loss(&self, state: &EndpointState) -> Option<f64> {
        if let Some(conn) = &state.cur_connection {
            let stats = conn.stats();
            if stats.path.sent_packets > 0 {
                let loss = stats.path.lost_packets as f64 / stats.path.sent_packets as f64;
                return Some(loss);
            }
        }
        None
    }
}

uniffi::setup_scaffolding!();