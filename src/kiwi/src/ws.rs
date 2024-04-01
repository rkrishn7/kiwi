use std::collections::BTreeMap;
use std::sync::Mutex;
use std::{net::SocketAddr, sync::Arc};

use anyhow::Context;
use arc_swap::ArcSwapOption;
use bytes::Bytes;
use fastwebsockets::{upgrade, CloseCode, FragmentCollector, Frame, Payload, WebSocketError};
use http::{Request, Response, StatusCode};
use http_body_util::Empty;
use hyper::service::service_fn;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::connection::ConnectionManager;
use crate::hook::authenticate::types::Authenticate;
use crate::hook::authenticate::types::Outcome;
use crate::hook::intercept::types::{AuthCtx, ConnectionCtx, WebSocketConnectionCtx};

use crate::hook::intercept::types::Intercept;
use crate::protocol::{Command, Message, ProtocolError};
use crate::source::{Source, SourceId};
use crate::tls::{tls_acceptor, MaybeTlsStream};

type Sources = Arc<Mutex<BTreeMap<SourceId, Box<dyn Source + Send + Sync + 'static>>>>;

/// Starts a WebSocket server with the specified configuration
pub async fn serve<I, A>(
    listen_addr: &SocketAddr,
    sources: Sources,
    intercept: Arc<ArcSwapOption<I>>,
    authenticate: Arc<ArcSwapOption<A>>,
    subscriber_config: crate::config::Subscriber,
    tls_config: Option<crate::config::Tls>,
    healthcheck: bool,
) -> anyhow::Result<()>
where
    I: Intercept + Send + Sync + 'static,
    A: Authenticate + Send + Sync + Unpin + 'static,
{
    let acceptor = if let Some(tls) = tls_config {
        Some(tls_acceptor(&tls.cert, &tls.key).context("Failed to build TLS acceptor")?)
    } else {
        None
    };
    let listener = tokio::net::TcpListener::bind(listen_addr).await?;
    tracing::info!("Server listening on: {listen_addr}");

    loop {
        let (stream, addr) = listener.accept().await?;
        tracing::debug!(addr = ?addr, "Accepted connection");
        let acceptor = acceptor.clone();
        let authenticate = Arc::clone(&authenticate);
        let intercept = Arc::clone(&intercept);
        let sources = Arc::clone(&sources);
        let subscriber_config = subscriber_config.clone();

        tokio::spawn(async move {
            let io = if let Some(acceptor) = acceptor {
                match acceptor.accept(stream).await {
                    Ok(stream) => hyper_util::rt::TokioIo::new(MaybeTlsStream::Tls(stream)),
                    Err(e) => {
                        tracing::error!(addr = ?addr, "Failed to accept TLS connection: {}", e);
                        return;
                    }
                }
            } else {
                hyper_util::rt::TokioIo::new(MaybeTlsStream::Plain(stream))
            };

            let builder =
                hyper_util::server::conn::auto::Builder::new(hyper_util::rt::TokioExecutor::new());
            let conn_fut = builder.serve_connection_with_upgrades(
                io,
                service_fn(move |req: Request<hyper::body::Incoming>| {
                    let authenticate = Arc::clone(&authenticate);
                    let sources = Arc::clone(&sources);
                    let intercept = Arc::clone(&intercept);
                    let subscriber_config = subscriber_config.clone();

                    async move {
                        if healthcheck && req.uri().path() == "/health" {
                            return Response::builder()
                                .status(StatusCode::OK)
                                .body(Empty::new());
                        }

                        let response = handle_ws(
                            sources,
                            intercept,
                            authenticate,
                            subscriber_config,
                            addr,
                            req,
                        )
                        .await;

                        Ok(response)
                    }
                }),
            );

            if let Err(e) = conn_fut.await {
                tracing::error!(addr = ?addr, "Error occurred while serving connection: {}", e);
            }
        });
    }
}

#[tracing::instrument(skip_all)]
async fn load_auth_ctx<A>(
    authenticate: Arc<ArcSwapOption<A>>,
    request: Request<hyper::body::Incoming>,
) -> Result<Option<AuthCtx>, ()>
where
    A: Authenticate + Send + Sync + Unpin + 'static,
{
    if let Some(hook) = authenticate.load().as_ref() {
        let outcome = hook.authenticate(request.map(|_| ())).await;

        match outcome {
            Ok(Outcome::Authenticate) => Ok(None),
            Ok(Outcome::WithContext(ctx)) => Ok(Some(AuthCtx::from_bytes(ctx))),
            outcome => {
                if outcome.is_err() {
                    tracing::error!(
                        "Failure occurred while running authentication hook: {:?}",
                        outcome.unwrap_err()
                    );
                }

                return Err(());
            }
        }
    } else {
        Ok(None)
    }
}

async fn handle_ws<I, A>(
    sources: Sources,
    intercept: Arc<ArcSwapOption<I>>,
    authenticate: Arc<ArcSwapOption<A>>,
    subscriber_config: crate::config::Subscriber,
    addr: SocketAddr,
    mut request: Request<hyper::body::Incoming>,
) -> Response<Empty<Bytes>>
where
    I: Intercept + Send + Sync + 'static,
    A: Authenticate + Send + Sync + Unpin + 'static,
{
    let (response, fut) = upgrade::upgrade(&mut request).expect("Failed to upgrade connection");

    let authenticate = Arc::clone(&authenticate);

    let auth_ctx = if let Ok(auth_ctx) = load_auth_ctx(authenticate, request).await {
        auth_ctx
    } else {
        return Response::builder()
            .status(StatusCode::UNAUTHORIZED)
            .body(Empty::new())
            .unwrap();
    };

    let connection_ctx = ConnectionCtx::WebSocket(WebSocketConnectionCtx { addr });

    tokio::spawn(async move {
        if let Err(e) = handle_client(
            fut,
            sources,
            intercept,
            subscriber_config,
            connection_ctx.clone(),
            auth_ctx,
        )
        .await
        {
            tracing::error!(
                addr = ?addr,
                "Error occurred while serving WebSocket client: {}",
                e
            );
        }

        tracing::debug!(connection = ?connection_ctx, "WebSocket connection terminated normally");
    });

    response
}

async fn handle_client<I>(
    fut: upgrade::UpgradeFut,
    sources: Sources,
    intercept: Arc<ArcSwapOption<I>>,
    subscriber_config: crate::config::Subscriber,
    connection_ctx: ConnectionCtx,
    auth_ctx: Option<AuthCtx>,
) -> anyhow::Result<()>
where
    I: Intercept + Send + Sync + 'static,
{
    let ws = fut.await?;
    let mut ws = fastwebsockets::FragmentCollector::new(ws);

    tracing::debug!(connection = ?connection_ctx, "WebSocket connection established");

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
        tokio::select! {
            biased;

            maybe_cmd = recv_cmd(&mut ws) => {
                match maybe_cmd {
                    Some(Ok(cmd)) => {
                        if cmd_tx.send(cmd).is_err() {
                            // If the send failed, the channel is closed thus we should
                            // terminate the connection
                            break;
                        }
                    }
                    Some(Err(e)) => {
                        let (close_code, reason) = match e {
                            RecvError::WebSocket(e) => {
                                match e {
                                    WebSocketError::ConnectionClosed => break,
                                    e => return Err(e.into()),
                                }
                            }
                            RecvError::Protocol(e) => {
                                match e {
                                    ProtocolError::CommandDeserialization(_) => {
                                        (CloseCode::Policy, e.to_string())
                                    }
                                    ProtocolError::UnsupportedCommandForm => {
                                        (CloseCode::Unsupported, e.to_string())
                                    }
                                }
                            },
                        };

                        let frame = Frame::close(close_code.into(), reason.as_bytes());
                        ws.write_frame(frame).await?;
                        break;
                    }
                    None => {
                        // The connection has been closed
                        break;
                    }
                }
            },
            msg = msg_rx.recv() => {
                match msg {
                    Some(msg) => {
                        let txt = serde_json::to_string(&msg).expect("failed to serialize message");

                        let frame = Frame::text(Payload::from(txt.as_bytes()));

                        ws.write_frame(frame).await?;

                    }
                    None => {
                        // The sole sender (our ingest actor) has hung up for some reason so we want to
                        // terminate the connection
                        break;
                    },
                }
            }
        }
    }

    Ok(())
}

enum RecvError {
    WebSocket(WebSocketError),
    Protocol(ProtocolError),
}

async fn recv_cmd<S>(ws: &mut FragmentCollector<S>) -> Option<Result<Command, RecvError>>
where
    S: AsyncReadExt + AsyncWriteExt + Unpin,
{
    let frame = match ws.read_frame().await {
        Ok(frame) => frame,
        Err(e) => {
            return Some(Err(RecvError::WebSocket(e)));
        }
    };

    match frame.opcode {
        fastwebsockets::OpCode::Text => {
            Some(
                serde_json::from_slice::<Command>(&frame.payload).map_err(|_| {
                    RecvError::Protocol(ProtocolError::CommandDeserialization(
                        // SAFETY: We know the payload is valid UTF-8 because `read_frame`
                        // guarantees that text frames payloads are valid UTF-8
                        unsafe { std::str::from_utf8_unchecked(&frame.payload) }.to_string(),
                    ))
                }),
            )
        }
        fastwebsockets::OpCode::Binary => Some(Err(RecvError::Protocol(
            ProtocolError::UnsupportedCommandForm,
        ))),
        fastwebsockets::OpCode::Close => None,
        _ => panic!("Received unexpected opcode"),
    }
}
