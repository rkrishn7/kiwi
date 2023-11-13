use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
/// A request that is sent from a client to the server
pub enum Command {
    Subscribe { topics: Vec<String> },
    Unsubscribe { topics: Vec<String> },
}

pub enum CommandResponse {}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
/// An info or error message that may be pushed to a client. A notice, in many
/// cases is not issued as a direct result of a command
pub enum Notice {
    Lag { topics: Vec<String>, count: u64 },
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", content = "data")]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
/// An outbound message that is sent from the server to a client
pub enum Message<T> {
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
