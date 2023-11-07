use tokio::sync::broadcast::{Receiver, Sender};

pub struct TopicBroadcastChannel<T> {
    pub tx: Sender<T>,
    _rx: Receiver<T>,
}

impl<T> From<(Sender<T>, Receiver<T>)> for TopicBroadcastChannel<T> {
    fn from(value: (Sender<T>, Receiver<T>)) -> Self {
        Self {
            tx: value.0,
            _rx: value.1,
        }
    }
}

impl<T> TopicBroadcastChannel<T> {
    pub fn subscribe(&self) -> Receiver<T> {
        self.tx.subscribe()
    }
}
