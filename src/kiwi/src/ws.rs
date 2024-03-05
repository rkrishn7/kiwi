use std::borrow::Cow;
use std::collections::BTreeMap;
use std::sync::Mutex;
use std::{net::SocketAddr, sync::Arc};

use arc_swap::ArcSwapOption;
use futures::{SinkExt, StreamExt};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::handshake::server::{ErrorResponse, Request, Response};
use tokio_tungstenite::tungstenite::http::StatusCode;
use tokio_tungstenite::WebSocketStream;

use crate::connection::ConnectionManager;
use crate::hook::authenticate::types::Authenticate;
use crate::hook::authenticate::types::Outcome;
use crate::hook::intercept::types::{AuthCtx, ConnectionCtx, WebSocketConnectionCtx};

use crate::hook::intercept::types::Intercept;
use crate::protocol::{Command, Message, ProtocolError as KiwiProtocolError};
use crate::source::{Source, SourceId};

use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::protocol::{CloseFrame, Message as ProtocolMessage};

/// Starts a WebSocket server with the specified configuration
pub async fn serve<I, A>(
    listen_addr: &SocketAddr,
    sources: Arc<Mutex<BTreeMap<SourceId, Box<dyn Source + Send + Sync + 'static>>>>,
    intercept: Arc<ArcSwapOption<I>>,
    authenticate: Arc<ArcSwapOption<A>>,
    subscriber_config: crate::config::Subscriber,
) -> anyhow::Result<()>
where
    I: Intercept + Send + Sync + 'static,
    A: Authenticate + Send + Sync + Unpin + 'static,
{
    // TODO: Support TLS
    let listener = TcpListener::bind(listen_addr).await?;
    tracing::info!("Server started. Listening on: {}", listen_addr);

    while let Ok((stream, addr)) = listener.accept().await {
        let sources = Arc::clone(&sources);
        let intercept = intercept.clone();
        let authenticate = authenticate.clone();
        let subscriber_config = subscriber_config.clone();
        tokio::spawn(async move {
            match perform_handshake(stream, authenticate).await {
                Ok((mut ws_stream, auth_ctx)) => {
                    tracing::info!(ip = ?addr, "New WebSocket connection");

                    let connection_ctx = ConnectionCtx::WebSocket(WebSocketConnectionCtx { addr });

                    let _ = drive_stream(
                        &mut ws_stream,
                        sources,
                        auth_ctx,
                        connection_ctx,
                        intercept,
                        subscriber_config,
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
    authenticate: Arc<ArcSwapOption<A>>,
) -> anyhow::Result<(WebSocketStream<S>, Option<AuthCtx>)>
where
    S: AsyncRead + AsyncWrite + Unpin,
    A: Authenticate + Unpin + Send + Sync + 'static,
{
    let mut auth_ctx = None;

    let handle = tokio::runtime::Handle::current();

    let ws_stream = tokio_tungstenite::accept_hdr_async_with_config(
        stream,
        |req: &Request, res: Response| {
            let request = req.clone();

            if let Some(hook) = authenticate.load().as_ref() {
                // The callback implemented by tokio-tungstenite is not async, which makes
                // it difficult to invoke async code from within it. Ultimately, we need
                // to block the current thread to run the async code. While it's not ideal,
                // tokio provides `tokio::task::block_in_place` to handle this.
                //
                // The tokio-tungstenite issue is tracked [here](https://github.com/snapview/tokio-tungstenite/issues/159)
                let outcome = tokio::task::block_in_place(move || {
                    handle.block_on(async { hook.authenticate(request).await })
                });

                match outcome {
                    Ok(Outcome::Authenticate) => (),
                    Ok(Outcome::WithContext(ctx)) => {
                        auth_ctx = Some(AuthCtx::from_bytes(ctx));
                    }
                    outcome => {
                        if outcome.is_err() {
                            tracing::error!(
                                "Failed to run authentication hook. Error {:?}",
                                outcome.unwrap_err()
                            );
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

async fn drive_stream<S, I>(
    stream: &mut WebSocketStream<S>,
    sources: Arc<Mutex<BTreeMap<SourceId, Box<dyn Source + Send + Sync + 'static>>>>,
    auth_ctx: Option<AuthCtx>,
    connection_ctx: ConnectionCtx,
    intercept: Arc<ArcSwapOption<I>>,
    subscriber_config: crate::config::Subscriber,
) -> anyhow::Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
    I: Intercept + Send + Sync + 'static,
{
    let (msg_tx, mut msg_rx) = tokio::sync::mpsc::unbounded_channel::<Message>();
    let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel::<Command>();

    let actor = ConnectionManager::new(
        sources,
        cmd_rx,
        msg_tx,
        connection_ctx.clone(),
        auth_ctx,
        intercept,
        subscriber_config,
    );

    // Spawn the ingest actor. If it terminates, the connection should be closed
    tokio::spawn(async move {
        if let Err(err) = actor.run().await {
            tracing::error!(connection = ?connection_ctx, "Connection manager terminated with error: {:?}", err);
        }
    });

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
