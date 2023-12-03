use std::net::SocketAddr;

use crate::event::EventPayload;

#[derive(Debug, Clone)]
pub enum Action {
    Forward,
    Discard,
    Transform(EventPayload),
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
}

#[derive(Debug, Clone)]
pub struct KafkaEventCtx {
    pub(crate) payload: EventPayload,
    pub(crate) topic: String,
    pub(crate) timestamp: Option<i64>,
    pub(crate) partition: i32,
    pub(crate) offset: i64,
}
