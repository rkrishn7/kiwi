use std::pin::Pin;

use async_stream::stream;
use futures::Stream;
use ringbuf::{HeapRb, Rb};
use tokio_stream::wrappers::{errors::BroadcastStreamRecvError, BroadcastStream};
use tokio_stream::StreamExt;

use crate::{protocol, source::SourceMessage, source::SourceResult};

#[derive(Debug, thiserror::Error)]
pub enum SubscriptionRecvError {
    #[error("Missed {0} messages from source due to process lag")]
    ProcessLag(u64),
    #[error("Missed {0} messages from source due to subscriber lag")]
    SubscriberLag(u64),
    #[error("Source closed")]
    SourceClosed,
}

pub enum Subscription {
    Pull(PullSubscription),
    Push(PushSubscription),
}

impl Subscription {
    pub fn from_mode(
        source_stream: BroadcastStream<SourceMessage>,
        mode: protocol::SubscriptionMode,
        buffer_capacity: Option<usize>,
    ) -> Self {
        match mode {
            protocol::SubscriptionMode::Pull => Self::Pull(PullSubscription {
                source_stream,
                requests: 0,
                lag: 0,
                buffer: buffer_capacity.map(HeapRb::new),
            }),
            protocol::SubscriptionMode::Push => Self::Push(PushSubscription { source_stream }),
        }
    }

    #[allow(dead_code)]
    fn as_pull(&mut self) -> &mut PullSubscription {
        match self {
            Subscription::Pull(state) => state,
            _ => panic!("Subscription is not in pull mode"),
        }
    }

    pub fn source_stream(
        &mut self,
    ) -> Pin<
        Box<
            dyn Stream<Item = Result<Vec<SourceMessage>, SubscriptionRecvError>> + '_ + Send + Sync,
        >,
    > {
        match self {
            Subscription::Pull(state) => Box::pin(state.source_stream()),
            Subscription::Push(state) => Box::pin(state.source_stream()),
        }
    }
}

pub struct PushSubscription {
    source_stream: BroadcastStream<SourceMessage>,
}

impl PushSubscription {
    pub fn source_stream(
        &mut self,
    ) -> impl Stream<Item = Result<Vec<SourceMessage>, SubscriptionRecvError>> + '_ {
        stream! {
            while let Some(message) = self.source_stream.next().await {
                yield message.map_err(|e| match e {
                    BroadcastStreamRecvError::Lagged(n) => SubscriptionRecvError::ProcessLag(n),
                }).map(|m| vec![m]);
            }

            yield Err(SubscriptionRecvError::SourceClosed);
        }
    }
}

pub struct PullSubscription {
    source_stream: BroadcastStream<SourceMessage>,
    requests: u64,
    lag: u64,
    buffer: Option<HeapRb<SourceResult>>,
}

impl PullSubscription {
    #[inline(always)]
    fn decrement_requests(&mut self) {
        self.requests -= 1;
    }

    #[inline(always)]
    pub fn add_requests(&mut self, n: u64) {
        self.requests += n;
    }

    #[inline(always)]
    fn has_requests(&self) -> bool {
        self.requests > 0
    }

    #[inline(always)]
    pub fn requests(&self) -> u64 {
        self.requests
    }

    #[inline(always)]
    fn reset_lag(&mut self) {
        self.lag = 0;
    }

    #[inline(always)]
    fn increment_lag(&mut self) {
        self.lag += 1;
    }

    pub fn source_stream(
        &mut self,
    ) -> impl Stream<Item = Result<Vec<SourceMessage>, SubscriptionRecvError>> + '_ {
        stream! {
            while let Some(message) = self.source_stream.next().await {
                if let Ok(SourceMessage::Result(result)) = message {
                    let first = match self.buffer.as_mut() {
                        Some(buffer) => buffer.push_overwrite(result),
                        None => Some(result),
                    };

                    if !self.has_requests() {
                        if first.is_some() {
                            self.increment_lag();
                            yield Err(SubscriptionRecvError::SubscriberLag(self.lag));
                        }
                    } else {
                        self.reset_lag();
                        let mut results = Vec::new();
                        if let Some(first) = first {
                            if self.has_requests() {
                                results.push(SourceMessage::Result(first));
                                self.decrement_requests();
                            }
                        }

                        if self.buffer.is_some() {
                            while self.has_requests() {
                                if let Some(result) = self.buffer.as_mut().and_then(|b| b.pop()) {
                                    results.push(SourceMessage::Result(result));
                                    self.decrement_requests();
                                } else {
                                    break;
                                }
                            }
                        }

                        yield Ok(results);
                    }
                } else {
                    yield message.map_err(|e| match e {
                        BroadcastStreamRecvError::Lagged(n) => SubscriptionRecvError::ProcessLag(n),
                    }).map(|m| vec![m]);
                }
            }

            yield Err(SubscriptionRecvError::SourceClosed);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::source::kafka::KafkaSourceResult;

    use super::*;
    use futures_util::FutureExt;
    use tokio::sync::broadcast;

    #[tokio::test]
    async fn test_push_subscription_yields_results() {
        let (tx, rx) = broadcast::channel(1);
        let mut subscription = Subscription::from_mode(
            BroadcastStream::new(rx),
            protocol::SubscriptionMode::Push,
            None,
        );
        let mut stream = subscription.source_stream();

        let message = SourceMessage::Result(SourceResult::Kafka(KafkaSourceResult {
            partition: 0,
            offset: 0,
            topic: "test".into(),
            key: None,
            payload: None,
            timestamp: None,
        }));

        tx.send(message).unwrap();

        let result = stream.next().await.unwrap().unwrap();
        assert_eq!(result.len(), 1);
        assert!(matches!(result[0], SourceMessage::Result(_)));
    }

    #[tokio::test]
    async fn test_push_subscription_notifies_source_dropped() {
        let (tx, rx) = broadcast::channel(1);
        let mut subscription = Subscription::from_mode(
            BroadcastStream::new(rx),
            protocol::SubscriptionMode::Push,
            None,
        );
        let mut stream = subscription.source_stream();

        drop(tx);

        let result = stream.next().await.unwrap();
        assert!(matches!(result, Err(SubscriptionRecvError::SourceClosed)));
    }

    #[tokio::test]
    async fn test_push_subscription_notifies_process_lag() {
        let (tx, rx) = broadcast::channel(1);
        let mut subscription = Subscription::from_mode(
            BroadcastStream::new(rx),
            protocol::SubscriptionMode::Push,
            None,
        );
        let mut stream = subscription.source_stream();

        for _ in 0..2 {
            let message = SourceMessage::Result(SourceResult::Kafka(KafkaSourceResult {
                partition: 0,
                offset: 0,
                topic: "test".into(),
                key: None,
                payload: None,
                timestamp: None,
            }));

            tx.send(message).unwrap();
        }

        let result = stream.next().await.unwrap();
        assert!(matches!(result, Err(SubscriptionRecvError::ProcessLag(1))));
    }

    #[tokio::test]
    async fn test_pull_subscription_notifies_source_dropped() {
        let (tx, rx) = broadcast::channel(1);
        let mut subscription = Subscription::from_mode(
            BroadcastStream::new(rx),
            protocol::SubscriptionMode::Pull,
            None,
        );
        let mut stream = subscription.source_stream();

        drop(tx);

        let result = stream.next().await.unwrap();
        assert!(matches!(result, Err(SubscriptionRecvError::SourceClosed)));
    }

    #[tokio::test]
    async fn test_pull_stream_buffers_results() {
        let (tx, rx) = broadcast::channel(10);
        let mut subscription = Subscription::from_mode(
            BroadcastStream::new(rx),
            protocol::SubscriptionMode::Pull,
            Some(5),
        );
        let mut stream = subscription.source_stream();

        for _ in 0..5 {
            let message = SourceMessage::Result(SourceResult::Kafka(KafkaSourceResult {
                partition: 0,
                offset: 0,
                topic: "test".into(),
                key: None,
                payload: None,
                timestamp: None,
            }));

            tx.send(message).unwrap();
        }

        let fut = tokio::time::timeout(std::time::Duration::from_millis(500), stream.next());

        // The buffer is at capacity, but there have been no requests
        // so nothing should be available to read
        assert!(matches!(fut.await, Err(_)));
    }

    #[tokio::test]
    async fn test_pull_stream_forwards_buffered_results() {
        let (tx, rx) = broadcast::channel(10);
        let mut subscription = Subscription::from_mode(
            BroadcastStream::new(rx),
            protocol::SubscriptionMode::Pull,
            Some(5),
        );

        for _ in 0..5 {
            let message = SourceMessage::Result(SourceResult::Kafka(KafkaSourceResult {
                partition: 0,
                offset: 0,
                topic: "test".into(),
                key: None,
                payload: None,
                timestamp: None,
            }));

            tx.send(message).unwrap();
        }

        {
            let mut stream = subscription.source_stream();
            for _ in 0..5 {
                let _ = stream.next().now_or_never();
            }
        }

        let pull = subscription.as_pull();

        pull.add_requests(5);

        let mut stream = subscription.source_stream();

        // Pass another message through the stream to trigger a subsequent poll
        let message = SourceMessage::Result(SourceResult::Kafka(KafkaSourceResult {
            partition: 0,
            offset: -1,
            topic: "test".into(),
            key: None,
            payload: None,
            timestamp: None,
        }));

        tx.send(message).unwrap();

        let result = stream.next().await.unwrap().unwrap();

        assert_eq!(result.len(), 5);

        for r in result {
            assert!(matches!(r, SourceMessage::Result(_)));
        }

        drop(stream);

        let pull = subscription.as_pull();

        // There should be no more remaining requests
        assert_eq!(pull.requests(), 0);

        // The buffer should contain the last message passed
        assert_eq!(pull.buffer.as_ref().unwrap().len(), 1);
        let result = pull.buffer.as_mut().unwrap().pop().unwrap();

        let KafkaSourceResult { offset, .. } = match result {
            SourceResult::Kafka(result) => result,
        };

        assert_eq!(offset, -1);
    }

    #[tokio::test]
    async fn test_pull_stream_no_buffer() {
        let (tx, rx) = broadcast::channel(10);
        let mut subscription = Subscription::from_mode(
            BroadcastStream::new(rx),
            protocol::SubscriptionMode::Pull,
            None,
        );

        for _ in 0..5 {
            let message = SourceMessage::Result(SourceResult::Kafka(KafkaSourceResult {
                partition: 0,
                offset: 0,
                topic: "test".into(),
                key: None,
                payload: None,
                timestamp: None,
            }));

            tx.send(message).unwrap();
        }

        let mut stream = subscription.source_stream();

        for i in 1..=5 {
            let next = stream.next().await.unwrap();
            assert!(matches!(
                next,
                Err(SubscriptionRecvError::SubscriberLag(lag)) if lag == i
            ));
        }

        drop(stream);

        let pull = subscription.as_pull();

        pull.add_requests(1);

        let message = SourceMessage::Result(SourceResult::Kafka(KafkaSourceResult {
            partition: 0,
            offset: 0,
            topic: "test".into(),
            key: None,
            payload: None,
            timestamp: None,
        }));

        tx.send(message).unwrap();

        let mut stream = subscription.source_stream();

        let next = stream.next().await.unwrap();

        assert!(matches!(next, Ok(_)));
        assert_eq!(next.unwrap().len(), 1);

        drop(stream);

        let pull = subscription.as_pull();

        // There should be no more remaining requests
        assert_eq!(pull.requests(), 0);
    }

    #[tokio::test]
    async fn test_pull_stream_requests() {
        let (tx, rx) = broadcast::channel(10);
        let mut subscription = Subscription::from_mode(
            BroadcastStream::new(rx),
            protocol::SubscriptionMode::Pull,
            Some(5),
        );

        for _ in 0..3 {
            let message = SourceMessage::Result(SourceResult::Kafka(KafkaSourceResult {
                partition: 0,
                offset: 0,
                topic: "test".into(),
                key: None,
                payload: None,
                timestamp: None,
            }));

            tx.send(message).unwrap();
        }

        {
            let mut stream = subscription.source_stream();
            for _ in 0..3 {
                let _ = stream.next().now_or_never();
            }
        }

        let pull = subscription.as_pull();

        pull.add_requests(1);

        let mut stream = subscription.source_stream();

        // Pass another message through the stream to trigger a subsequent poll
        let message = SourceMessage::Result(SourceResult::Kafka(KafkaSourceResult {
            partition: 0,
            offset: -1,
            topic: "test".into(),
            key: None,
            payload: None,
            timestamp: None,
        }));

        tx.send(message).unwrap();

        let result = stream.next().await.unwrap().unwrap();

        assert_eq!(result.len(), 1);

        for r in result {
            assert!(matches!(r, SourceMessage::Result(_)));
        }

        drop(stream);

        let pull = subscription.as_pull();

        // There should be no more remaining requests
        assert_eq!(pull.requests(), 0);

        assert_eq!(pull.buffer.as_ref().unwrap().len(), 3);
    }

    #[tokio::test]
    async fn test_pull_stream_sends_subscriber_lag_notices() {
        let (tx, rx) = broadcast::channel(10);
        let mut subscription = Subscription::from_mode(
            BroadcastStream::new(rx),
            protocol::SubscriptionMode::Pull,
            Some(5),
        );
        let mut stream = subscription.source_stream();

        for _ in 0..6 {
            let message = SourceMessage::Result(SourceResult::Kafka(KafkaSourceResult {
                partition: 0,
                offset: 0,
                topic: "test".into(),
                key: None,
                payload: None,
                timestamp: None,
            }));

            tx.send(message).unwrap();
        }

        assert!(matches!(
            stream.next().await,
            Some(Err(SubscriptionRecvError::SubscriberLag(1)))
        ));

        let message = SourceMessage::Result(SourceResult::Kafka(KafkaSourceResult {
            partition: 0,
            offset: 0,
            topic: "test".into(),
            key: None,
            payload: None,
            timestamp: None,
        }));

        tx.send(message).unwrap();

        assert!(matches!(
            stream.next().await,
            Some(Err(SubscriptionRecvError::SubscriberLag(2)))
        ));
    }
}
