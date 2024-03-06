use tokio::sync::broadcast::Receiver;

use crate::hook;

use self::{counter::CounterSourceBuilder, kafka::KafkaSourceBuilder};

pub mod counter;
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
    Counter(counter::CounterSourceResult),
}

pub enum SourceMetadata {
    Kafka(kafka::KafkaSourceMetadata),
}

#[derive(Debug, thiserror::Error)]
pub enum SubscribeError {
    #[error("Finite source has ended")]
    FiniteSourceEnded,
}

pub trait Source {
    fn subscribe(&mut self) -> Result<Receiver<SourceMessage>, SubscribeError>;

    fn source_id(&self) -> &SourceId;

    fn metadata_tx(&self) -> &Option<tokio::sync::mpsc::UnboundedSender<SourceMetadata>>;

    fn as_any(&self) -> &dyn std::any::Any;
}

pub type SourceId = String;

impl From<SourceResult> for hook::intercept::types::EventCtx {
    fn from(value: SourceResult) -> Self {
        match value {
            SourceResult::Kafka(kafka_result) => Self::Kafka(kafka_result.into()),
            SourceResult::Counter(counter_result) => Self::Counter(counter_result.into()),
        }
    }
}

pub struct SourceBuilder;

impl KafkaSourceBuilder for SourceBuilder {}
impl CounterSourceBuilder for SourceBuilder {}
