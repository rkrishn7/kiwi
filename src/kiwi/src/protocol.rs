use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::source::SourceId;

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
/// A request that is sent from a client to the server
pub enum Command {
    Subscribe { sources: Vec<SourceId> },
    Unsubscribe { sources: Vec<SourceId> },
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CommandResponse {
    SubscribeOk {
        sources: Vec<SourceId>,
    },
    UnsubscribeOk {
        sources: Vec<SourceId>,
    },
    SubscribeError {
        sources: Vec<SourceId>,
        error: String,
    },
    UnsubscribeError {
        sources: Vec<SourceId>,
        error: String,
    },
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
/// An info or error message that may be pushed to a client. A notice, in many
/// cases is not issued as a direct result of a command
pub enum Notice {
    Lag { source: SourceId, count: u64 },
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", content = "data")]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
/// An outbound message that is sent from the server to a client
pub enum Message<T> {
    CommandResponse(CommandResponse),
    Notice(Notice),
    Result(T),
}

#[derive(Debug, Error)]
pub enum ProtocolError {
    #[error("Unsupported command form. Only UTF-8 encoded text is supported")]
    UnsupportedCommandForm,
    #[error("Encountered an error while deserializing the command payload {0}")]
    CommandDeserialization(String),
}
