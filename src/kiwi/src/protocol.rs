use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::source::{self, SourceId};

/// The subscription mode to use for a source subscription
#[derive(Debug, Default, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SubscriptionMode {
    /// Pull subscriptions require the client to request events from the source
    Pull,
    /// Push subscriptions send events to the client as they are produced
    #[default]
    Push,
}

/// Commands are issued by kiwi clients to the server
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "type")]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Command {
    /// Subscribe to the specified source
    #[serde(rename_all = "camelCase")]
    Subscribe {
        /// The ID for the source to subscribe to
        source_id: SourceId,
        /// The subscription mode to use
        #[serde(default)]
        mode: SubscriptionMode,
    },
    /// Unsubscribe from the specified source
    #[serde(rename_all = "camelCase")]
    Unsubscribe {
        /// The ID for the source to unsubscribe from. The source must be
        /// associated with an active subscription for the request to be valid
        source_id: SourceId,
    },
    /// Request the next `n` events from the source. This is only valid for
    /// pull-based subscriptions
    #[serde(rename_all = "camelCase")]
    Request {
        /// The ID of the source to request data from
        source_id: SourceId,
        /// The (additive) number of events to request
        n: u64,
    },
}

/// Command responses are issued by the server to clients in response to
/// commands
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
    #[serde(rename_all = "camelCase")]
    RequestOk { source_id: SourceId, requests: u64 },
    #[serde(rename_all = "camelCase")]
    RequestError { source_id: SourceId, error: String },
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

impl From<source::SourceResult> for Message {
    fn from(value: source::SourceResult) -> Self {
        Message::Result(value.into())
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "sourceType", rename_all = "camelCase")]
pub enum SourceResult {
    #[serde(rename_all = "camelCase")]
    Kafka {
        #[serde(with = "crate::util::serde::base64")]
        /// Event key
        key: Option<Vec<u8>>,
        #[serde(with = "crate::util::serde::base64")]
        /// base64 encoded event payload
        payload: Option<Vec<u8>>,
        /// Source ID this event was produced from
        source_id: SourceId,
        /// Timestamp at which the message was produced
        timestamp: Option<i64>,
        /// Partition ID this event was produced from
        partition: i32,
        /// Offset at which the message was produced
        offset: i64,
    },
    #[serde(rename_all = "camelCase")]
    Counter {
        /// Source ID this counter event was produced from
        source_id: SourceId,
        /// Event count
        count: u64,
    },
}

impl From<source::SourceResult> for SourceResult {
    fn from(value: source::SourceResult) -> Self {
        match value {
            source::SourceResult::Kafka(kafka) => Self::Kafka {
                key: kafka.key,
                payload: kafka.payload,
                source_id: kafka.topic,
                partition: kafka.partition,
                offset: kafka.offset,
                timestamp: kafka.timestamp,
            },
            source::SourceResult::Counter(counter) => Self::Counter {
                source_id: counter.source_id,
                count: counter.count,
            },
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
                source_id: "test".into(),
                mode: SubscriptionMode::Push
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

        let message = Message::Result(SourceResult::Kafka {
            payload: Some("test".into()),
            source_id: "test".into(),
            key: None,
            timestamp: None,
            partition: 0,
            offset: 1,
        });

        let serialized = serde_json::to_string(&message).unwrap();
        let encoded = base64::engine::general_purpose::STANDARD.encode("test".as_bytes());
        assert_eq!(
            serialized,
            r#"{"type":"RESULT","data":{"sourceType":"kafka","key":null,"payload":"$encoded","sourceId":"test","timestamp":null,"partition":0,"offset":1}}"#.replace("$encoded", encoded.as_str())
        );

        let message = Message::Result(SourceResult::Counter {
            source_id: "test".into(),
            count: 1,
        });

        let serialized = serde_json::to_string(&message).unwrap();
        assert_eq!(
            serialized,
            r#"{"type":"RESULT","data":{"sourceType":"counter","sourceId":"test","count":1}}"#
        );
    }
}
