//! Bridge between WIT types and local plugin types
use super::bindgen::kiwi::kiwi::intercept_types::*;
use crate::hook::intercept::types;
use crate::util::macros::try_conv_bail;

impl From<types::Context> for Context {
    fn from(value: types::Context) -> Self {
        Self {
            auth: value.auth.map(|a| a.raw),
            connection: value.connection.into(),
            event: value.event.into(),
        }
    }
}

impl From<types::EventCtx> for EventCtx {
    fn from(value: types::EventCtx) -> Self {
        match value {
            types::EventCtx::Kafka(ctx) => Self::Kafka(ctx.into()),
        }
    }
}

impl From<types::KafkaEventCtx> for KafkaEventCtx {
    fn from(value: types::KafkaEventCtx) -> Self {
        let timestamp: Option<u64> = value
            .timestamp
            .map(|ts| try_conv_bail!(ts, "timestamp conversion must not fail"));
        let partition = try_conv_bail!(value.partition, "partition conversion must not fail");
        let offset = try_conv_bail!(value.offset, "offset conversion must not fail");
        Self {
            payload: value.payload,
            topic: value.topic,
            timestamp,
            partition,
            offset,
        }
    }
}

impl From<types::ConnectionCtx> for ConnectionCtx {
    fn from(value: types::ConnectionCtx) -> Self {
        match value {
            types::ConnectionCtx::WebSocket(ctx) => Self::Websocket(ctx.into()),
        }
    }
}

impl From<types::WebSocketConnectionCtx> for Websocket {
    fn from(value: types::WebSocketConnectionCtx) -> Self {
        Self {
            addr: Some(value.addr.to_string()),
        }
    }
}

impl From<Action> for types::Action {
    fn from(value: Action) -> Self {
        match value {
            Action::Forward => Self::Forward,
            Action::Discard => Self::Discard,
            Action::Transform(payload) => Self::Transform(payload),
        }
    }
}
