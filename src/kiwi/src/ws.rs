use std::borrow::Cow;
use std::collections::BTreeMap;
use std::{net::SocketAddr, sync::Arc};

use futures::{SinkExt, StreamExt};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::handshake::server::{ErrorResponse, Request, Response};
use tokio_tungstenite::tungstenite::http::StatusCode;
use tokio_tungstenite::WebSocketStream;

use crate::hook::authenticate::types::Outcome;
use crate::hook::authenticate::Authenticate;
use crate::hook::intercept::types::{AuthCtx, ConnectionCtx, WebSocketConnectionCtx};
use crate::ingest::IngestActor;

use crate::hook::intercept::Intercept;
use crate::protocol::{Command, Message, ProtocolError as KiwiProtocolError};
use crate::source::Source;

use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::protocol::{CloseFrame, Message as ProtocolMessage};

/// Starts a WebSocket server with the specified configuration
pub async fn serve<S, I, A>(
    listen_addr: &SocketAddr,
    sources: Arc<BTreeMap<String, S>>,
    intercept_hook: Option<I>,
    authenticate_hook: Option<A>,
) -> anyhow::Result<()>
where
    S: Source + Send + Sync + 'static,
    I: Intercept + Clone + Send + Sync + 'static,
    A: Authenticate + Clone + Send + Sync + Unpin + 'static,
{
    // TODO: Support TLS
    let listener = TcpListener::bind(listen_addr).await?;
    tracing::info!("Server started. Listening on: {}", listen_addr);

    while let Ok((stream, addr)) = listener.accept().await {
        let sources = Arc::clone(&sources);
        let intercept_hook = intercept_hook.clone();
        let authenticate_hook = authenticate_hook.clone();
        tokio::spawn(async move {
            match perform_handshake(stream, authenticate_hook).await {
                Ok((mut ws_stream, auth_ctx)) => {
                    tracing::info!("New WebSocket connection: {}", addr);

                    let connection_ctx = ConnectionCtx::WebSocket(WebSocketConnectionCtx { addr });

                    let _ = drive_stream(
                        &mut ws_stream,
                        sources,
                        auth_ctx,
                        connection_ctx,
                        intercept_hook,
                    )
                    .await;
                }
                Err(e) => {
                    tracing::error!(
                        "Error during websocket handshake occurred for client {}: {}",
                        addr,
                        e
                    );
                }
            }

            tracing::info!("{} disconnected", addr);
        });
    }

    Ok(())
}

async fn perform_handshake<S, A>(
    stream: S,
    authenticate_hook: Option<A>,
) -> anyhow::Result<(WebSocketStream<S>, Option<AuthCtx>)>
where
    S: AsyncRead + AsyncWrite + Unpin,
    A: Authenticate + Unpin + Send + Sync + 'static,
{
    let mut auth_ctx = None;
    let ws_stream = tokio_tungstenite::accept_hdr_async_with_config(
        stream,
        |req: &Request, res: Response| {
            if let Some(hook) = authenticate_hook {
                let outcome = tokio::task::block_in_place(|| hook.authenticate(req));

                match outcome {
                    Ok(Outcome::Authenticate) => (),
                    Ok(Outcome::WithContext(ctx)) => {
                        auth_ctx = Some(AuthCtx::from_bytes(ctx));
                    }
                    outcome => {
                        if outcome.is_err() {
                            tracing::error!("Failed to run authentication hook");
                        }

                        let mut res = ErrorResponse::new(Some("Unauthorized".to_string()));
                        *res.status_mut() = StatusCode::UNAUTHORIZED;
                        return Err(res);
                    }
                }
            }

            Ok(res)
        },
        None,
    )
    .await?;

    Ok((ws_stream, auth_ctx))
}

async fn drive_stream<Stream, S, I>(
    stream: &mut WebSocketStream<Stream>,
    sources: Arc<BTreeMap<String, S>>,
    auth_ctx: Option<AuthCtx>,
    connection_ctx: ConnectionCtx,
    intercept_hook: Option<I>,
) -> anyhow::Result<()>
where
    Stream: AsyncRead + AsyncWrite + Unpin,
    S: Source + Send + Sync + 'static,
    I: Intercept + Clone + Send + Sync + 'static,
{
    let (msg_tx, mut msg_rx) = tokio::sync::mpsc::unbounded_channel::<Message>();
    let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel::<Command>();

    let actor = IngestActor::new(
        sources,
        cmd_rx,
        msg_tx,
        connection_ctx,
        auth_ctx,
        intercept_hook,
    );

    // Spawn the ingest actor. If it terminates, the connection should be closed
    tokio::spawn(actor.run());

    loop {
        // It's important that we bias the select block towards the WebSocket stream
        // because message pushes are likely to occur much more frequently than
        // commands, and we want to ensure the polling of each future remains fair.
        tokio::select! {
            biased;

            Some(protocol_msg) = stream.next() => {
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
                            ProtocolMessage::Close(_) => {
                                break;
                            },
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

                                if let Err(e) = stream.close(Some(close_frame)).await {
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
                        let txt = serde_json::to_string(&msg).expect("failed to serialize message");

                        // TODO: Batch here and flush on an interval?
                        if let Err(e) = stream.send(ProtocolMessage::Text(txt)).await {
                            tracing::error!("Encountered unexpected error while feeding data to WS stream: {}", e);
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

    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::{
        hook::intercept::{types::Action, wasm::WasmInterceptHook},
        source::kafka::KafkaTopicSource,
    };

    use super::*;
    use futures::stream::FusedStream;
    use std::{
        collections::BTreeMap,
        task::{Context, Poll},
    };
    use tokio::io::{AsyncWriteExt, ReadBuf};
    use tokio_tungstenite::tungstenite::protocol::Role;
    use tokio_tungstenite::WebSocketStream;

    use std::{io, io::Cursor, pin::Pin};

    struct TestInterceptHook {}

    impl Intercept for TestInterceptHook {
        fn intercept(&self, _: &crate::hook::intercept::types::Context) -> anyhow::Result<Action> {
            Ok(Action::Forward)
        }
    }

    #[tokio::test]
    #[ignore]
    async fn test_invalid_command() {
        let (_, mut incoming) = tokio::io::duplex(1024);
        incoming
            .write(&[
                0x89, 0x02, 0x01, 0x02, 0x8a, 0x01, 0x03, 0x01, 0x07, 0x48, 0x65, 0x6c, 0x6c, 0x6f,
                0x2c, 0x20, 0x80, 0x06, 0x57, 0x6f, 0x72, 0x6c, 0x64, 0x21, 0x82, 0x03, 0x01, 0x02,
                0x03,
            ])
            .await
            .unwrap();
        // let incoming = Cursor::new(Vec::with_capacity(1024));
        let mut stream = WebSocketStream::from_raw_socket(incoming, Role::Client, None).await;

        let sources: BTreeMap<String, KafkaTopicSource> = BTreeMap::new();
        let auth_ctx = None;
        let connection_ctx = ConnectionCtx::WebSocket(WebSocketConnectionCtx {
            addr: "127.0.0.1:3000".parse().unwrap(),
        });

        // stream
        //     .send(tokio_tungstenite::tungstenite::protocol::Message::Text(
        //         serde_json::to_string(&Command::Subscribe {
        //             source_id: "test".into(),
        //         })
        //         .unwrap(),
        //     ))
        //     .await
        //     .unwrap();

        tokio::select! {
            biased;

            _ = drive_stream(
                &mut stream,
                Arc::new(sources),
                auth_ctx,
                connection_ctx,
                None::<WasmInterceptHook>,
            ) => (),
            _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => {
                panic!("Expected stream to terminate");
            }
        }

        // TODO: Assert the stream was closed with the appropriate error

        assert!(stream.is_terminated());
    }
}
