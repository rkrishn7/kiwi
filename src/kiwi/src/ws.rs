use std::collections::BTreeMap;
use std::sync::Mutex;
use std::{net::SocketAddr, sync::Arc};

use arc_swap::ArcSwapOption;
use axum::body::Body;
use axum::extract::{ConnectInfo, Request, State};
use axum::{response::IntoResponse, routing::get, Router};
use fastwebsockets::{upgrade, CloseCode, FragmentCollector, Frame, Payload};
use http::{Response, StatusCode};
use http_body_util::Empty;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use crate::connection::ConnectionManager;
use crate::hook::authenticate::types::Authenticate;
use crate::hook::authenticate::types::Outcome;
use crate::hook::intercept::types::{AuthCtx, ConnectionCtx, WebSocketConnectionCtx};

use crate::hook::intercept::types::Intercept;
use crate::protocol::{Command, Message, ProtocolError as KiwiProtocolError};
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
) -> anyhow::Result<()>
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

    let app = Router::new()
        .route("/", get(ws_handler))
        .route("/health", get(healthcheck))
        .with_state(Arc::new(state));

    // TODO: Support TLS
    let listener = TcpListener::bind(listen_addr).await?;

    tracing::info!("Server listening on: {}", listen_addr);

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;

    Ok(())
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

    let auth_ctx = if let Some(hook) = authenticate.load().as_ref() {
        let outcome = hook.authenticate(request.map(|_| ())).await;

        match outcome {
            Ok(Outcome::Authenticate) => None,
            Ok(Outcome::WithContext(ctx)) => Some(AuthCtx::from_bytes(ctx)),
            outcome => {
                if outcome.is_err() {
                    tracing::error!(
                        "Failed to run authentication hook. Error {:?}",
                        outcome.unwrap_err()
                    );
                }

                let mut res = Response::new(Empty::new());
                *res.status_mut() = StatusCode::UNAUTHORIZED;
                return res;
            }
        }
    } else {
        None
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
            connection_ctx,
            auth_ctx,
        )
        .await
        {
            tracing::error!("Failed to handle WebSocket client: {}", e);
        }
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
                        // Gracefully handle protocol errors by sending a close frame
                        if let Some(protocol_err) = e.downcast_ref::<KiwiProtocolError>() {
                            let close_frame = Frame::close(CloseCode::Protocol.into(), protocol_err.to_string().as_bytes());
                            ws.write_frame(close_frame).await?;

                            break;
                        }

                        return Err(e);
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

async fn recv_cmd<S>(ws: &mut FragmentCollector<S>) -> Option<anyhow::Result<Command>>
where
    S: AsyncReadExt + AsyncWriteExt + Unpin,
{
    let frame = match ws.read_frame().await {
        Ok(frame) => frame,
        Err(e) => {
            return Some(Err(e.into()));
        }
    };

    let maybe_cmd = match frame.opcode {
        fastwebsockets::OpCode::Text => {
            Some(
                serde_json::from_slice::<Command>(&frame.payload).map_err(|_| {
                    KiwiProtocolError::CommandDeserialization(
                        // SAFETY: We know the payload is valid UTF-8 because `read_frame`
                        // guarantees that text frames payloads are valid UTF-8
                        unsafe { std::str::from_utf8_unchecked(&frame.payload) }.to_string(),
                    )
                }),
            )
        }
        fastwebsockets::OpCode::Binary => Some(Err(KiwiProtocolError::UnsupportedCommandForm)),
        _ => None,
    };

    maybe_cmd.map(|cmd| cmd.map_err(Into::into))
}
