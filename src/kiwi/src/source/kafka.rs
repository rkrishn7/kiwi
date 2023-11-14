use std::collections::BTreeMap;

use futures::stream::StreamExt;
use futures::{future::Fuse, FutureExt};
use maplit::btreemap;
use rdkafka::Message;
use rdkafka::{
    consumer::{Consumer, StreamConsumer},
    message::OwnedMessage,
    ClientConfig,
};
use tokio::sync::{
    broadcast::{Receiver, Sender},
    oneshot,
};

use crate::event::MutableEvent;
use crate::plugin;

use super::{Source, SourceId};

#[derive(Debug)]
pub struct KafkaTopicSource {
    topic: String,
    _shutdown_trigger: oneshot::Sender<()>,
    tx: Sender<OwnedMessage>,
}

impl Source for KafkaTopicSource {
    type Message = OwnedMessage;

    fn subscribe(&self) -> Receiver<Self::Message> {
        self.tx.subscribe()
    }

    fn source_id(&self) -> &SourceId {
        &self.topic
    }
}

/// A Kafka consumer that subscribes to a single topic. It is "lazy" because it
/// only begins consuming from the specified topic once there is a subscriber present.
struct LazyKafkaTopicConsumer {
    tx: Sender<OwnedMessage>,
    topic: String,
    inner: StreamConsumer,
    shutdown_rx: Fuse<oneshot::Receiver<()>>,
    state: LazyKafkaTopicConsumerState,
}

enum LazyKafkaTopicConsumerState {
    AwaitingSubscribers,
    Consuming,
    Completed,
}

impl LazyKafkaTopicConsumer {
    pub fn new(
        inner: StreamConsumer,
        topic: String,
        tx: Sender<OwnedMessage>,
        shutdown_rx: Fuse<oneshot::Receiver<()>>,
    ) -> Self {
        Self {
            tx,
            topic,
            inner,
            shutdown_rx,
            state: LazyKafkaTopicConsumerState::AwaitingSubscribers,
        }
    }

    pub async fn run(&mut self) {
        loop {
            match &self.state {
                LazyKafkaTopicConsumerState::AwaitingSubscribers => {
                    if self.tx.receiver_count() > 0 {
                        println!("receiver count {}", self.tx.receiver_count());
                        self.state = LazyKafkaTopicConsumerState::Consuming;
                        continue;
                    }

                    // TODO: This seems bad, there's no need to do this if we get notification
                    // of when a new subscriber is present
                    tokio::task::yield_now().await;
                }
                LazyKafkaTopicConsumerState::Consuming => {
                    self.inner.subscribe(&[&self.topic]).unwrap_or_else(|_| panic!("failed to subscribe to kafka topic {}", &self.topic));

                    let mut stream = self.inner.stream();

                    loop {
                        tokio::select! {
                            _ = &mut self.shutdown_rx => break,
                            next = stream.next() => {
                                match next {
                                    Some(message) => {
                                        match message {
                                            Err(err) => {
                                                tracing::error!(
                                                    "Encountered Kafka error while yielding messages: {}",
                                                    err
                                                );
                                            }
                                            Ok(borrowed_message) => {
                                                // Erroring does not mean future calls will fail, since new subscribers
                                                // may be created. If there are no subscribers, we simply discard the message
                                                // and move on
                                                let _ = self.tx.send(borrowed_message.detach());
                                            }
                                        };
                                    },
                                    None => break,
                                }
                            }
                        }
                    }

                    self.state = LazyKafkaTopicConsumerState::Completed;
                }
                LazyKafkaTopicConsumerState::Completed => break,
            }
        }
    }
}

impl KafkaTopicSource {
    pub fn new(topic: String, client_config: &ClientConfig) -> anyhow::Result<Self> {
        // TODO: make this capacity configurable
        let (tx, _) = tokio::sync::broadcast::channel::<OwnedMessage>(100);
        let (shutdown_trigger, shutdown_rx) = oneshot::channel::<()>();

        let consumer: StreamConsumer = client_config.create()?;

        let mut lazy_consumer =
            LazyKafkaTopicConsumer::new(consumer, topic.clone(), tx.clone(), shutdown_rx.fuse());

        // TODO: Run multiple consumers per topic. Configurable? Based on topic metadata?

        tokio::task::spawn(async move {
            lazy_consumer.run().await;
        });

        Ok(Self {
            topic,
            _shutdown_trigger: shutdown_trigger,
            tx,
        })
    }
}

impl From<OwnedMessage> for plugin::types::EventCtx {
    fn from(value: OwnedMessage) -> Self {
        Self::Kafka(plugin::types::KafkaEventCtx {
            payload: value.payload().map(|p| p.to_owned()),
            topic: value.topic().to_string(),
            timestamp: value.timestamp().to_millis(),
            partition: value.partition(),
            offset: value.offset(),
        })
    }
}

impl MutableEvent for OwnedMessage {
    fn set_payload(self, payload: Option<Vec<u8>>) -> Self {
        OwnedMessage::set_payload(self, payload)
    }
}

pub fn build_sources(
    topics: impl Iterator<Item = String>,
    group_prefix: String,
    bootstrap_servers: Vec<String>,
) -> BTreeMap<SourceId, KafkaTopicSource> {
    let mut client_config = ClientConfig::new();
    let group_id = format!("{}{}", group_prefix, nanoid::nanoid!());
    let bootstrap_servers = bootstrap_servers.join(",");

    client_config.extend(btreemap! {
        // The group ID must be unique for each kiwi process. Kafka uses consumer
        // groups for partition assignments among members in the group. Since downstream
        // susbcribers to kiwi (e.g. WebSocket connections) have no context on what
        // partitions are, there must be a 1:1 relationship between each process and consumer
        // group to ensure all messages are received.

        // Note that this is has performance implications for topics that see high production
        // rates.
        "group.id".to_string() => group_id.to_string(),
        // We don't care about offset committing, since we are just relaying the latest messages.
        "enable.auto.commit".to_string() => "false".to_string(),
        "enable.partition.eof".to_string() => "false".to_string(),
        // Always start from the latest topic. We could make this configurable in the future
        "auto.offset.reset".to_string() =>  "latest".to_string(),
        // A friendly label to present to Kafka
        "client.id".to_string() => "kiwi".to_string(),
        "bootstrap.servers".to_string() => bootstrap_servers,
        "session.timeout.ms".to_string() => 30000.to_string(),
    });

    topics
        .map(|topic| {
            let k_source = KafkaTopicSource::new(topic, &client_config)
                .expect("Failed to create Kafka topic source");

            (k_source.source_id().clone(), k_source)
        })
        .collect::<BTreeMap<String, KafkaTopicSource>>()
}
