use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::source::{self, SourceId};

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
    Lag {
        source: SourceId,
        count: u64,
    },
    SubscriptionClosed {
        source: SourceId,
        message: Option<String>,
    },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type", content = "data")]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
/// An outbound message that is sent from the server to a client
pub enum Message {
    CommandResponse(CommandResponse),
    Notice(Notice),
    Result(SourceResult),
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SourceResult {
    #[serde(with = "crate::util::serde::base64")]
    /// Event key
    pub key: Option<Vec<u8>>,
    #[serde(with = "crate::util::serde::base64")]
    /// base64 encoded event payload
    pub payload: Option<Vec<u8>>,
    /// Source ID this event was produced from
    pub source_id: SourceId,
    /// Type of source this event was produced from
    pub source_type: String,
    /// Source-specific metadata in JSON format
    pub metadata: Option<String>,
}

impl From<source::SourceResult> for SourceResult {
    fn from(value: source::SourceResult) -> Self {
        match value {
            source::SourceResult::Kafka(kafka) => {
                let metadata = serde_json::json!({
                    "partition": kafka.partition,
                    "offset": kafka.offset,
                    "timestamp": kafka.timestamp,
                });

                Self {
                    key: kafka.key,
                    payload: kafka.payload,
                    source_id: kafka.topic,
                    source_type: "kafka".into(),
                    metadata: Some(metadata.to_string()),
                }
            }
        }
    }
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
    use base64::Engine;

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
        let message: Message = Message::CommandResponse(CommandResponse::SubscribeOk {
            source_id: "test".into(),
        });

        let serialized = serde_json::to_string(&message).unwrap();
        assert_eq!(
            serialized,
            r#"{"type":"COMMAND_RESPONSE","data":{"type":"SUBSCRIBE_OK","sourceId":"test"}}"#
        );

        let message: Message = Message::CommandResponse(CommandResponse::UnsubscribeOk {
            source_id: "test".into(),
        });

        let serialized = serde_json::to_string(&message).unwrap();
        assert_eq!(
            serialized,
            r#"{"type":"COMMAND_RESPONSE","data":{"type":"UNSUBSCRIBE_OK","sourceId":"test"}}"#
        );

        let message: Message = Message::CommandResponse(CommandResponse::SubscribeError {
            source_id: "test".into(),
            error: "test".into(),
        });

        let serialized = serde_json::to_string(&message).unwrap();
        assert_eq!(
            serialized,
            r#"{"type":"COMMAND_RESPONSE","data":{"type":"SUBSCRIBE_ERROR","sourceId":"test","error":"test"}}"#
        );

        let message: Message = Message::CommandResponse(CommandResponse::UnsubscribeError {
            source_id: "test".into(),
            error: "test".into(),
        });

        let serialized = serde_json::to_string(&message).unwrap();
        assert_eq!(
            serialized,
            r#"{"type":"COMMAND_RESPONSE","data":{"type":"UNSUBSCRIBE_ERROR","sourceId":"test","error":"test"}}"#
        );

        let message: Message = Message::Notice(Notice::Lag {
            source: "test".into(),
            count: 1,
        });

        let serialized = serde_json::to_string(&message).unwrap();
        assert_eq!(
            serialized,
            r#"{"type":"NOTICE","data":{"type":"LAG","source":"test","count":1}}"#
        );

        let message: Message = Message::Notice(Notice::SubscriptionClosed {
            source: "test".into(),
            message: Some("New partition added".to_string()),
        });

        let serialized = serde_json::to_string(&message).unwrap();
        assert_eq!(
            serialized,
            r#"{"type":"NOTICE","data":{"type":"SUBSCRIPTION_CLOSED","source":"test","message":"New partition added"}}"#
        );

        let message = Message::Result(SourceResult {
            payload: Some("test".into()),
            source_id: "test".into(),
            source_type: "kafka".into(),
            key: None,
            metadata: None,
        });

        let serialized = serde_json::to_string(&message).unwrap();
        let encoded = base64::engine::general_purpose::STANDARD.encode("test".as_bytes());
        assert_eq!(
            serialized,
            r#"{"type":"RESULT","data":{"key":null,"payload":"$encoded","source_id":"test","source_type":"kafka","metadata":null}}"#.replace("$encoded", encoded.as_str())
        );
    }
}
