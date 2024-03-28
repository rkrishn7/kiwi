use std::collections::BTreeMap;
use std::sync::Mutex;
use std::{net::SocketAddr, sync::Arc};

use arc_swap::ArcSwapOption;
use axum::body::Body;
use axum::extract::{ConnectInfo, Request, State};
use axum::{response::IntoResponse, routing::get, Router};
use axum_server::tls_rustls::RustlsConfig;
use fastwebsockets::{upgrade, CloseCode, FragmentCollector, Frame, Payload, WebSocketError};
use http::{Response, StatusCode};
use http_body_util::Empty;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::connection::ConnectionManager;
use crate::hook::authenticate::types::Authenticate;
use crate::hook::authenticate::types::Outcome;
use crate::hook::intercept::types::{AuthCtx, ConnectionCtx, WebSocketConnectionCtx};

use crate::hook::intercept::types::Intercept;
use crate::protocol::{Command, Message, ProtocolError};
use crate::source::{Source, SourceId};

type Sources = Arc<Mutex<BTreeMap<SourceId, Box<dyn Source + Send + Sync + 'static>>>>;

struct AppState<I, A> {
    sources: Sources,
    intercept: Arc<ArcSwapOption<I>>,
    authenticate: Arc<ArcSwapOption<A>>,
    subscriber_config: crate::config::Subscriber,
}

/// Starts a WebSocket server with the specified configuration
pub async fn serve<I, A>(
    listen_addr: &SocketAddr,
    sources: Sources,
    intercept: Arc<ArcSwapOption<I>>,
    authenticate: Arc<ArcSwapOption<A>>,
    subscriber_config: crate::config::Subscriber,
    tls_config: Option<crate::config::Tls>,
) -> anyhow::Result<()>
where
    I: Intercept + Send + Sync + 'static,
    A: Authenticate + Send + Sync + Unpin + 'static,
{
    let app = make_app(sources, intercept, authenticate, subscriber_config);
    let svc = app.into_make_service_with_connect_info::<SocketAddr>();

    tracing::info!("Server listening on: {}", listen_addr);

    if let Some(tls) = tls_config {
        let config = RustlsConfig::from_pem_file(&tls.cert, &tls.key).await?;

        axum_server::bind_rustls(*listen_addr, config)
            .serve(svc)
            .await?;
    } else {
        axum_server::bind(*listen_addr).serve(svc).await?;
    };

    Ok(())
}

fn make_app<I, A>(
    sources: Sources,
    intercept: Arc<ArcSwapOption<I>>,
    authenticate: Arc<ArcSwapOption<A>>,
    subscriber_config: crate::config::Subscriber,
) -> Router
where
    I: Intercept + Send + Sync + 'static,
    A: Authenticate + Send + Sync + Unpin + 'static,
{
    let state = AppState {
        sources,
        intercept,
        authenticate,
        subscriber_config,
    };

    Router::new()
        .route("/", get(ws_handler))
        .route("/health", get(healthcheck))
        .with_state(Arc::new(state))
}

#[tracing::instrument(skip_all)]
async fn load_auth_ctx<A>(
    authenticate: Arc<ArcSwapOption<A>>,
    request: Request<Body>,
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

async fn ws_handler<I, A>(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<Arc<AppState<I, A>>>,
    mut request: Request<Body>,
) -> impl IntoResponse
where
    I: Intercept + Send + Sync + 'static,
    A: Authenticate + Send + Sync + Unpin + 'static,
{
    let (response, fut) = upgrade::upgrade(&mut request).expect("failed to build upgrade response");

    let authenticate = Arc::clone(&state.authenticate);

    let auth_ctx = if let Ok(auth_ctx) = load_auth_ctx(authenticate, request).await {
        auth_ctx
    } else {
        return Response::builder()
            .status(StatusCode::UNAUTHORIZED)
            .body(Empty::new())
            .unwrap();
    };

    let intercept = Arc::clone(&state.intercept);
    let sources = Arc::clone(&state.sources);
    let subscriber_config = state.subscriber_config.clone();
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

        tracing::debug!(connection = ?connection_ctx, "WebSocket connection terminated");
    });

    response
}

async fn healthcheck() -> impl IntoResponse {
    "OK"
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

            Some(cmd) = recv_cmd(&mut ws) => {
                match cmd {
                    Ok(cmd) => {
                        if cmd_tx.send(cmd).is_err() {
                            // If the send failed, the channel is closed thus we should
                            // terminate the connection
                            break;
                        }
                    }
                    Err(e) => {
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
        _ => None,
    }
}
