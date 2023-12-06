use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::source::SourceId;

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "type")]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
/// A request that is sent from a client to the server
pub enum Command {
    #[serde(rename_all = "camelCase")]
    Subscribe { source_id: SourceId },
    #[serde(rename_all = "camelCase")]
    Unsubscribe { source_id: SourceId },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type")]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CommandResponse {
    #[serde(rename_all = "camelCase")]
    SubscribeOk { source_id: SourceId },
    #[serde(rename_all = "camelCase")]
    UnsubscribeOk { source_id: SourceId },
    #[serde(rename_all = "camelCase")]
    SubscribeError { source_id: SourceId, error: String },
    #[serde(rename_all = "camelCase")]
    UnsubscribeError { source_id: SourceId, error: String },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type")]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
/// An info or error message that may be pushed to a client. A notice, in many
/// cases is not issued as a direct result of a command
pub enum Notice {
    Lag { source: SourceId, count: u64 },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_de() {
        let command = r#"{"type":"SUBSCRIBE","sourceId":"test"}"#;
        let deserialized: Command = serde_json::from_str(command).unwrap();
        assert_eq!(
            deserialized,
            Command::Subscribe {
                source_id: "test".into()
            }
        );

        let command = r#"{"type":"UNSUBSCRIBE","sourceId":"test"}"#;
        let deserialized: Command = serde_json::from_str(command).unwrap();
        assert_eq!(
            deserialized,
            Command::Unsubscribe {
                source_id: "test".into()
            }
        );
    }

    #[test]
    fn test_message_ser() {
        let message: Message<()> = Message::CommandResponse(CommandResponse::SubscribeOk {
            source_id: "test".into(),
        });

        let serialized = serde_json::to_string(&message).unwrap();
        assert_eq!(
            serialized,
            r#"{"type":"COMMAND_RESPONSE","data":{"type":"SUBSCRIBE_OK","sourceId":"test"}}"#
        );

        let message: Message<()> = Message::CommandResponse(CommandResponse::UnsubscribeOk {
            source_id: "test".into(),
        });

        let serialized = serde_json::to_string(&message).unwrap();
        assert_eq!(
            serialized,
            r#"{"type":"COMMAND_RESPONSE","data":{"type":"UNSUBSCRIBE_OK","sourceId":"test"}}"#
        );

        let message: Message<()> = Message::CommandResponse(CommandResponse::SubscribeError {
            source_id: "test".into(),
            error: "test".into(),
        });

        let serialized = serde_json::to_string(&message).unwrap();
        assert_eq!(
            serialized,
            r#"{"type":"COMMAND_RESPONSE","data":{"type":"SUBSCRIBE_ERROR","sourceId":"test","error":"test"}}"#
        );

        let message: Message<()> = Message::CommandResponse(CommandResponse::UnsubscribeError {
            source_id: "test".into(),
            error: "test".into(),
        });

        let serialized = serde_json::to_string(&message).unwrap();
        assert_eq!(
            serialized,
            r#"{"type":"COMMAND_RESPONSE","data":{"type":"UNSUBSCRIBE_ERROR","sourceId":"test","error":"test"}}"#
        );

        let message: Message<()> = Message::Notice(Notice::Lag {
            source: "test".into(),
            count: 1,
        });

        let serialized = serde_json::to_string(&message).unwrap();
        assert_eq!(
            serialized,
            r#"{"type":"NOTICE","data":{"type":"LAG","source":"test","count":1}}"#
        );

        #[derive(Serialize)]
        struct SourceResult {
            topic: String,
        }

        let message = Message::Result(SourceResult {
            topic: "test".into(),
        });

        let serialized = serde_json::to_string(&message).unwrap();
        assert_eq!(serialized, r#"{"type":"RESULT","data":{"topic":"test"}}"#);
    }
}
