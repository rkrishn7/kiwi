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
        lag_notice_threshold: Option<u64>,
    ) -> Self {
        match mode {
            protocol::SubscriptionMode::Pull => Self::Pull(PullSubscription {
                source_stream,
                requests: 0,
                lag: 0,
                buffer: buffer_capacity.map(|cap| HeapRb::new(cap)),
                lag_notice_threshold,
            }),
            protocol::SubscriptionMode::Push => Self::Push(PushSubscription { source_stream }),
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
        }
    }
}

pub struct PullSubscription {
    source_stream: BroadcastStream<SourceMessage>,
    requests: u64,
    lag: u64,
    buffer: Option<HeapRb<SourceResult>>,
    lag_notice_threshold: Option<u64>,
}

impl PullSubscription {
    #[inline(always)]
    fn remove_requests(&mut self, n: u64) {
        self.requests = self.requests.saturating_sub(n);
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
    fn should_emit_lag_notice(&self) -> bool {
        self.lag_notice_threshold
            .map(|t| self.lag >= t)
            .unwrap_or(false)
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
                            }
                        }

                        if self.buffer.is_some() {
                            while self.has_requests() {
                                if let Some(result) = self.buffer.as_mut().and_then(|b| b.pop()) {
                                    results.push(SourceMessage::Result(result));
                                } else {
                                    break;
                                }
                            }
                        }

                        self.remove_requests(results.len() as u64);
                        yield Ok(results);
                    }
                } else {
                    yield message.map_err(|e| match e {
                        BroadcastStreamRecvError::Lagged(n) => SubscriptionRecvError::ProcessLag(n),
                    }).map(|m| vec![m]);
                }
            }
        }
    }
}
