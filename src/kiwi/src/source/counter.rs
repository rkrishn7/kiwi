use std::sync::{Arc, Weak};

use futures_util::{future::Fuse, FutureExt};
use tokio::sync::broadcast::{Receiver, Sender};

use crate::hook;

use super::{Source, SourceId, SourceMessage, SourceMetadata, SourceResult, SubscribeError};

type ShutdownTrigger = tokio::sync::oneshot::Sender<()>;
type ShutdownReceiver = tokio::sync::oneshot::Receiver<()>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CounterSourceResult {
    pub source_id: String,
    pub count: u64,
}

impl From<CounterSourceResult> for hook::intercept::types::CounterEventCtx {
    fn from(value: CounterSourceResult) -> Self {
        Self {
            count: value.count,
            source_id: value.source_id,
        }
    }
}

pub struct CounterSource {
    id: String,
    tx: Weak<Sender<SourceMessage>>,
    initial_subscription_tx: Option<tokio::sync::oneshot::Sender<()>>,
    _shutdown_trigger: ShutdownTrigger,
}

impl CounterSource {
    pub fn new(
        id: String,
        min: u64,
        max: Option<u64>,
        interval: std::time::Duration,
        lazy: bool,
    ) -> Self {
        let (tx, _) = tokio::sync::broadcast::channel(1_000);
        let (shutdown_trigger, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
        let (initial_subscription_tx, initial_subscription_rx) =
            tokio::sync::oneshot::channel::<()>();

        let tx = Arc::new(tx);

        // The counter task should be the only thing holding a strong reference
        // to the sender. Handing the source a weak reference to the sender
        // allows downstream subscriptions to properly detect when the source
        // has ended. This is important for finite sources.
        let weak_tx = Arc::downgrade(&tx);

        let task = CounterTask {
            source_id: id.clone(),
            min,
            max,
            interval,
            tx,
            lazy,
            initial_subscription_rx,
            shutdown_rx: shutdown_rx.fuse(),
        };

        tokio::spawn(task.run());

        Self {
            id,
            _shutdown_trigger: shutdown_trigger,
            tx: weak_tx,
            initial_subscription_tx: Some(initial_subscription_tx),
        }
    }
}

impl Source for CounterSource {
    fn subscribe(&mut self) -> Result<Receiver<SourceMessage>, SubscribeError> {
        if let Some(tx) = self.initial_subscription_tx.take() {
            let _ = tx.send(());
        }

        // If the counter task (our sole sender) has ended, it indicates that
        // the source has ended
        if let Some(tx) = self.tx.upgrade() {
            Ok(tx.subscribe())
        } else {
            Err(SubscribeError::FiniteSourceEnded)
        }
    }

    fn source_id(&self) -> &SourceId {
        &self.id
    }

    fn metadata_tx(&self) -> &Option<tokio::sync::mpsc::UnboundedSender<SourceMetadata>> {
        &None
    }
}

pub struct CounterTask {
    source_id: String,
    min: u64,
    max: Option<u64>,
    interval: std::time::Duration,
    tx: Arc<Sender<SourceMessage>>,
    lazy: bool,
    initial_subscription_rx: tokio::sync::oneshot::Receiver<()>,
    shutdown_rx: Fuse<ShutdownReceiver>,
}

impl CounterTask {
    pub async fn run(mut self) {
        let mut current = self.min;

        if self.lazy {
            let _ = self.initial_subscription_rx.await;
        }

        let mut interval = tokio::time::interval(self.interval);

        loop {
            tokio::select! {
                _ = &mut self.shutdown_rx => break,
                _ = interval.tick() => {
                    if let Some(max) = self.max {
                        if current > max {
                            break;
                        }
                    }

                    let _ = self.tx.send(SourceMessage::Result(SourceResult::Counter(CounterSourceResult {
                        source_id: self.source_id.clone(),
                        count: current,
                    })));

                    current += 1;
                }
            }
        }

        tracing::debug!("Counter task for source {} shutting down", self.source_id);
    }
}

pub trait CounterSourceBuilder {
    fn build_source(
        id: String,
        min: u64,
        max: Option<u64>,
        interval: std::time::Duration,
        lazy: bool,
    ) -> Box<dyn Source + Send + Sync + 'static> {
        Box::new(CounterSource::new(id, min, max, interval, lazy))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_subscribing_fails_after_source_ended() {
        let mut source = CounterSource::new(
            "test".into(),
            0,
            Some(3),
            std::time::Duration::from_millis(5),
            false,
        );

        tokio::time::sleep(std::time::Duration::from_millis(25)).await;

        assert!(matches!(
            source.subscribe(),
            Err(SubscribeError::FiniteSourceEnded)
        ));
    }

    #[tokio::test]
    async fn test_lazy_starts_after_first_subscription() {
        let mut source = CounterSource::new(
            "test".into(),
            0,
            Some(3),
            std::time::Duration::from_millis(1),
            true,
        );

        let mut rx = source.subscribe().unwrap();

        let msg = rx.recv().await.unwrap();

        assert!(matches!(
            msg,
            SourceMessage::Result(SourceResult::Counter(CounterSourceResult {
                source_id,
                count
            })) if source_id == "test" && count == 0,
        ));
    }
}
