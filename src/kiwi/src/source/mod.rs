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

pub trait Source {
    fn subscribe(&self) -> Receiver<SourceMessage>;

    fn source_id(&self) -> &SourceId;
}

pub type SourceId = String;
