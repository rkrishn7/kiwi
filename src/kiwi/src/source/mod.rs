use base64::Engine;
use rdkafka::Message;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast::Receiver;

pub mod kafka;

#[derive(Clone, Debug)]
pub enum SourceMessage<T> {
    /// A source-specific event
    Result(T),
    /// Source metadata has changed
    MetadataChanged(String),
}

pub trait Source {
    type Result;

    fn subscribe(&self) -> Receiver<SourceMessage<Self::Result>>;

    fn source_id(&self) -> &SourceId;
}

pub type SourceId = String;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SourceResult {
    /// Event payload
    pub payload: Option<String>,
    /// Source ID
    pub source_id: SourceId,
    /// Timestamp at which the message was produced
    pub timestamp: Option<i64>,
}

impl From<rdkafka::message::OwnedMessage> for SourceResult {
    fn from(value: rdkafka::message::OwnedMessage) -> Self {
        let payload = value
            .payload()
            .map(|p| base64::engine::general_purpose::STANDARD.encode(p));

        Self {
            payload,
            source_id: value.topic().to_owned(),
            timestamp: value.timestamp().to_millis(),
        }
    }
}
