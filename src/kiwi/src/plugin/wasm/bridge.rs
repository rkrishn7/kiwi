//! Bridge between WIT types and local plugin types
use super::bindgen::kiwi::kiwi::types::*;
use crate::{plugin, util::macros::try_conv_bail};

impl From<plugin::types::Context> for Context {
    fn from(value: plugin::types::Context) -> Self {
        Self {
            connection: value.connection.into(),
            event: value.event.into(),
        }
    }
}

impl From<plugin::types::EventCtx> for EventCtx {
    fn from(value: plugin::types::EventCtx) -> Self {
        match value {
            plugin::types::EventCtx::Kafka(ctx) => Self::Kafka(ctx.into()),
        }
    }
}

impl From<plugin::types::KafkaEventCtx> for KafkaEventCtx {
    fn from(value: plugin::types::KafkaEventCtx) -> Self {
        let timestamp: Option<u64> = value
            .timestamp
            .map(|ts| try_conv_bail!(ts, "timestamp conversion must not fail"));
        let partition = try_conv_bail!(value.partition, "partition conversion must not fail");
        let offset = try_conv_bail!(value.offset, "offset conversion must not fail");
        Self {
            payload: value.payload.into(),
            topic: value.topic,
            timestamp,
            partition,
            offset,
        }
    }
}

impl From<plugin::types::ConnectionCtx> for ConnectionCtx {
    fn from(value: plugin::types::ConnectionCtx) -> Self {
        match value {
            plugin::types::ConnectionCtx::WebSocket(ctx) => Self::Websocket(ctx.into()),
        }
    }
}

impl From<plugin::types::WebSocketConnectionCtx> for Websocket {
    fn from(value: plugin::types::WebSocketConnectionCtx) -> Self {
        Self {
            auth: value.auth.map(|a| a.into()),
            addr: Some(value.addr.to_string()),
        }
    }
}

impl From<plugin::types::AuthCtx> for Auth {
    fn from(value: plugin::types::AuthCtx) -> Self {
        match value {
            plugin::types::AuthCtx::Jwt(ctx) => Self::Jwt(ctx.into()),
        }
    }
}

impl From<plugin::types::JwtCtx> for Jwt {
    fn from(value: plugin::types::JwtCtx) -> Self {
        Self {
            claims: value
                .claims
                .into_iter()
                .map(|(k, v)| (k.into(), v.into()))
                .collect(),
        }
    }
}

impl From<Action> for plugin::types::Action {
    fn from(value: Action) -> Self {
        match value {
            Action::Forward => Self::Forward,
            Action::Discard => Self::Discard,
            Action::Transform(payload) => Self::Transform(payload),
        }
    }
}
