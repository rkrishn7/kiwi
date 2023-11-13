use tokio::sync::broadcast::Receiver;

pub mod kafka;

pub trait Source {
    type Message;

    fn subscribe(&self) -> Receiver<Self::Message>;

    fn source_id(&self) -> &SourceId;
}

pub type SourceId = String;
