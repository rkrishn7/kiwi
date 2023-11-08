use std::borrow::Cow;
use std::{collections::HashMap, net::SocketAddr, sync::Arc};

use base64::{
    alphabet,
    engine::{self, general_purpose},
    Engine as _,
};
use futures::{FutureExt, SinkExt, StreamExt};
use rdkafka::message::OwnedMessage;
use rdkafka::Message as _Message;
use tokio::net::TcpListener;
use tokio::sync::broadcast::Sender;
use tokio::sync::oneshot;

use thiserror::Error;

use crate::ingest::IngestActor;

use crate::message::{Command, Message};

use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::protocol::{CloseFrame, Message as ProtocolMessage};

#[derive(Debug, Error)]
enum ConnectionError {
    #[error("Unsupported command form. Only UTF-8 encoded text is supported")]
    UnsupportedCommandForm,
    #[error("Encountered an error while deserializing the command payload {0}")]
    CommandDeserialization(String),
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct MessageData {
    payload: Option<String>,
    topic: String,
    timestamp: Option<i64>,
    partition: i32,
    offset: i64,
}

impl From<OwnedMessage> for MessageData {
    fn from(value: OwnedMessage) -> Self {
        Self {
            payload: value.payload().map(|p| general_purpose::STANDARD.encode(p)),
            topic: value.topic().to_owned(),
            timestamp: value.timestamp().to_millis(),
            partition: value.partition(),
            offset: value.offset(),
        }
    }
}

/// Starts a WebSocket server with the specified configuration
pub async fn serve<T>(
    listen_addr: &SocketAddr,
    topic_senders: Arc<HashMap<String, Sender<T>>>,
) -> anyhow::Result<()>
where
    T: Clone + Send + 'static,
    MessageData: From<T>,
{
    // TODO: Support TLS
    let listener = TcpListener::bind(listen_addr).await?;
    tracing::info!("Server started. Listening on: {}", listen_addr);

    while let Ok((stream, addr)) = listener.accept().await {
        let topic_senders = Arc::clone(&topic_senders);
        tokio::spawn(async move {
            match tokio_tungstenite::accept_async(stream).await {
                Ok(mut ws_stream) => {
                    tracing::info!("New WebSocket connection: {}", addr);

                    let (msg_tx, mut msg_rx) =
                        tokio::sync::mpsc::unbounded_channel::<Message<MessageData>>();
                    let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel::<Command>();
                    let (_shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
                    let shutdown_tripwire = shutdown_rx.fuse();

                    let actor = IngestActor::new(topic_senders, cmd_rx, msg_tx, shutdown_tripwire);

                    tokio::spawn(actor.run());

                    loop {
                        // It's important that we bias the select block towards the WebSocket stream
                        // because message pushes are likely to occur much more frequently than
                        // commands, and we want to ensure the polling of each future remains fair.
                        tokio::select! {
                            biased;

                            Some(protocol_msg) = ws_stream.next() => {
                                match protocol_msg {
                                    Ok(msg) => {
                                        let cmd = match msg {
                                            ProtocolMessage::Text(text) => {
                                                match serde_json::from_str::<Command>(&text) {
                                                    Ok(cmd) => Ok(cmd),
                                                    Err(_) => Err(ConnectionError::CommandDeserialization(text)),
                                                }
                                            },
                                            ProtocolMessage::Binary(_) => Err(ConnectionError::UnsupportedCommandForm),
                                            // Handling of Ping/Close messages are delegated to tungstenite
                                            _ => continue,
                                        };

                                        match cmd {
                                            Ok(cmd) => {
                                                if let Err(_) = cmd_tx.send(cmd) {
                                                    // If the send failed, the channel is closed
                                                    // thus we should terminate the connection
                                                    break;
                                                }
                                            },
                                            Err(e) => {
                                                let close_frame = CloseFrame {
                                                    code: CloseCode::Unsupported,
                                                    reason: Cow::from(e.to_string()),
                                                };

                                                if let Err(e) = ws_stream.close(Some(close_frame)).await {
                                                    tracing::error!("Failed to gracefully close the WebSocket connection with error {}", e);
                                                }

                                                break;
                                            },
                                        }
                                    },
                                    Err(tokio_tungstenite::tungstenite::Error::ConnectionClosed) => break,
                                    Err(e) => {
                                        tracing::error!("Encountered unexpected error while reading from WS stream: {}", e);
                                        break;
                                    }
                                }
                            },
                            Some(msg) = msg_rx.recv() => {
                                let txt = serde_json::to_string(&msg).expect("yeet");

                                if let Err(e) = ws_stream.send(ProtocolMessage::Text(txt)).await {
                                    tracing::error!("Encountered unexpected error while writing to WS stream: {}", e);
                                }
                            },
                            else => break,
                        }
                    }
                }
                Err(err) => {
                    tracing::error!(
                        "Error during websocket handshake occurred for client {}: {}",
                        addr,
                        err
                    );
                }
            }
        });
    }

    Ok(())
}
