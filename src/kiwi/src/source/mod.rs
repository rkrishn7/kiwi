use tokio::sync::broadcast::Receiver;

pub mod kafka;

#[derive(Clone, Debug)]
pub enum SourceMessage {
    /// A source-specific event
    Result(SourceResult),
    /// Source metadata has changed
    MetadataChanged(String),
}

#[derive(Debug, Clone)]
pub enum SourceResult {
    Kafka(kafka::KafkaSourceResult),
}

impl From<SourceResult> for SourceMessage {
    fn from(value: SourceResult) -> Self {
        SourceMessage::Result(value)
    }
}

impl TryFrom<SourceMessage> for SourceResult {
    type Error = SourceMessage;

    fn try_from(value: SourceMessage) -> Result<Self, Self::Error> {
        match value {
            SourceMessage::Result(result) => Ok(result),
            _ => Err(value),
        }
    }
}

pub enum SourceMetadata {
    Kafka(kafka::KafkaSourceMetadata),
}

pub trait Source {
    fn subscribe(&self) -> Receiver<SourceMessage>;

    fn source_id(&self) -> &SourceId;

    fn metadata_tx(&self) -> &Option<tokio::sync::mpsc::UnboundedSender<SourceMetadata>>;
}

pub type SourceId = String;
