use std::net::SocketAddr;

use async_trait::async_trait;

#[derive(Debug, Clone)]
pub enum TransformedPayload {
    Kafka(Option<Vec<u8>>),
    Counter(u64),
}

#[derive(Debug, Clone)]
pub enum Action {
    Forward,
    Discard,
    Transform(TransformedPayload),
}

#[derive(Debug, Clone)]
/// Context needed to execute a plugin
pub struct Context {
    pub(crate) auth: Option<AuthCtx>,
    pub(crate) connection: ConnectionCtx,
    pub(crate) event: EventCtx,
}

#[derive(Debug, Clone)]
pub struct AuthCtx {
    pub(crate) raw: Vec<u8>,
}

impl AuthCtx {
    pub fn from_bytes(raw: Vec<u8>) -> Self {
        Self { raw }
    }
}

#[derive(Debug, Clone)]
pub enum ConnectionCtx {
    WebSocket(WebSocketConnectionCtx),
}

#[derive(Debug, Clone)]
pub struct WebSocketConnectionCtx {
    pub(crate) addr: SocketAddr,
}

#[derive(Debug, Clone)]
pub enum EventCtx {
    Kafka(KafkaEventCtx),
    Counter(CounterEventCtx),
}

#[derive(Debug, Clone)]
pub struct KafkaEventCtx {
    pub(crate) payload: Option<Vec<u8>>,
    pub(crate) topic: String,
    pub(crate) timestamp: Option<i64>,
    pub(crate) partition: i32,
    pub(crate) offset: i64,
}

#[derive(Debug, Clone)]
pub struct CounterEventCtx {
    pub(crate) source_id: String,
    pub(crate) count: u64,
}

#[async_trait]
pub trait Intercept {
    async fn intercept(&self, context: &Context) -> anyhow::Result<Action>;
}
