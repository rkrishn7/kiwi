use tokio::sync::broadcast::Receiver;

pub mod kafka;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SourceMessage {
    /// A source-specific event
    Result(SourceResult),
    /// Source metadata has changed
    MetadataChanged(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceResult {
    Kafka(kafka::KafkaSourceResult),
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
