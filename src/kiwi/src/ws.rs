use std::borrow::Cow;
use std::collections::BTreeMap;
use std::{net::SocketAddr, sync::Arc};

use base64::{engine::general_purpose, Engine as _};
use futures::{SinkExt, StreamExt};
use rdkafka::message::OwnedMessage;
use rdkafka::Message as _Message;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::handshake::server::Callback;

use crate::event::MutableEvent;
use crate::ingest::IngestActor;

use crate::hook::intercept::{self, Intercept};
use crate::protocol::{Command, Message, ProtocolError as KiwiProtocolError};
use crate::source::Source;

use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::protocol::{CloseFrame, Message as ProtocolMessage};

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
pub async fn serve<S, M, I>(
    listen_addr: &SocketAddr,
    sources: Arc<BTreeMap<String, S>>,
    intercept_hook: Option<I>,
) -> anyhow::Result<()>
where
    S: Source<Message = M> + Send + Sync + 'static,
    M: Into<intercept::types::EventCtx>
        + Into<MessageData>
        + MutableEvent
        + std::fmt::Debug
        + Clone
        + Send
        + 'static,
    I: Intercept + Clone + Send + Sync + 'static,
{
    // TODO: Support TLS
    let listener = TcpListener::bind(listen_addr).await?;
    tracing::info!("Server started. Listening on: {}", listen_addr);

    while let Ok((stream, addr)) = listener.accept().await {
        let sources = Arc::clone(&sources);
        let pre_forward = intercept_hook.clone();
        tokio::spawn(async move {
            match tokio_tungstenite::accept_async(stream).await {
                Ok(mut ws_stream) => {
                    tracing::info!("New WebSocket connection: {}", addr);

                    let (msg_tx, mut msg_rx) =
                        tokio::sync::mpsc::unbounded_channel::<Message<MessageData>>();
                    let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel::<Command>();

                    let ctx = intercept::types::ConnectionCtx::WebSocket(
                        intercept::types::WebSocketConnectionCtx { addr },
                    );

                    let actor = IngestActor::new(sources, cmd_rx, msg_tx, ctx, pre_forward);

                    // Spawn the ingest actor. If it terminates, the connection should be closed
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
                                                    Err(_) => Err(KiwiProtocolError::CommandDeserialization(text)),
                                                }
                                            },
                                            ProtocolMessage::Binary(_) => Err(KiwiProtocolError::UnsupportedCommandForm),
                                            // Handling of Ping/Close messages are delegated to tungstenite
                                            _ => continue,
                                        };

                                        match cmd {
                                            Ok(cmd) => {
                                                if cmd_tx.send(cmd).is_err() {
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
                            msg = msg_rx.recv() => {
                                match msg {
                                    Some(msg) => {
                                        let txt = serde_json::to_string(&msg).expect("yeet");

                                        // TODO: We don't want to use `send` here as it implies flushing
                                        if let Err(e) = ws_stream.send(ProtocolMessage::Text(txt)).await {
                                            tracing::error!("Encountered unexpected error while writing to WS stream: {}", e);
                                        }
                                    }
                                    None => {
                                        // The sole sender (our ingest actor) has hung up for some reason so we want to
                                        // terminate the connection
                                        break;
                                    },
                                }
                            },
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

pub struct JwtVerifyCallback<'a> {
    algorithm_type: jwt::AlgorithmType,
    pem: &'a [u8],
}

impl<'a> Callback for JwtVerifyCallback<'a> {
    fn on_request(
        self,
        request: &tokio_tungstenite::tungstenite::handshake::server::Request,
        response: tokio_tungstenite::tungstenite::handshake::server::Response,
    ) -> Result<
        tokio_tungstenite::tungstenite::handshake::server::Response,
        tokio_tungstenite::tungstenite::handshake::server::ErrorResponse,
    > {
        todo!()
    }
}

async fn consume_stream<S>(stream: S, addr: SocketAddr) -> anyhow::Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let callback = JwtVerifyCallback {
        algorithm_type: jwt::AlgorithmType::Hs256,
        pem: b"secret",
    };

    let mut ws_stream =
        tokio_tungstenite::accept_hdr_async_with_config(stream, callback, None).await?;
    tracing::info!("New WebSocket connection: {}", addr);

    todo!()
}
