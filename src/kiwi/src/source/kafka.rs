use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Context;
use futures::stream::StreamExt;
use futures::{future::Fuse, FutureExt};
use maplit::btreemap;
use rdkafka::client::{Client, DefaultClientContext};
use rdkafka::util::Timeout;
use rdkafka::{
    consumer::{Consumer, StreamConsumer},
    message::OwnedMessage,
    ClientConfig,
};
use rdkafka::{Message, TopicPartitionList};
use tokio::sync::{
    broadcast::{Receiver, Sender},
    oneshot,
};

use crate::event::MutableEvent;
use crate::hook;

use super::{Source, SourceId, SourceMessage};

pub struct PartitionConsumer {
    consumer: StreamConsumer,
    shutdown_rx: Fuse<oneshot::Receiver<()>>,
    tx: Sender<SourceMessage<OwnedMessage>>,
}

impl PartitionConsumer {
    pub fn new<'a>(
        topic: &'a str,
        partition: i32,
        offset: rdkafka::Offset,
        client_config: &'a ClientConfig,
        shutdown_rx: Fuse<oneshot::Receiver<()>>,
        tx: Sender<SourceMessage<OwnedMessage>>,
    ) -> anyhow::Result<Self> {
        let consumer: StreamConsumer = client_config.create().context(format!(
            "Failed to create stream consumer for topic/partition {}/{}",
            topic, partition,
        ))?;

        let mut tpl = TopicPartitionList::new();

        tpl.add_partition_offset(topic, partition, offset)
            .context(format!(
                "Failed to add topic/partition/offset {}/{}/{:?}",
                topic, partition, offset
            ))?;

        consumer.assign(&tpl).context(format!(
            "Failed to assign topic/partition {}/{} to stream consumer",
            topic, partition
        ))?;

        Ok(Self {
            consumer,
            shutdown_rx,
            tx,
        })
    }

    pub async fn run(mut self) {
        let mut stream = self.consumer.stream();

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
                                    // An error here does not mean future calls will fail, since new subscribers
                                    // may be created. If there are no subscribers, we simply discard the message
                                    // and move on
                                    let _ = self.tx.send(SourceMessage::Result(borrowed_message.detach()));
                                }
                            };
                        },
                        None => break,
                    }
                }
            }
        }
    }
}

type ShutdownTrigger = oneshot::Sender<()>;

pub struct KafkaTopicSource {
    topic: String,
    // Map of partition ID -> shutdown trigger
    _partition_consumers: Arc<Mutex<BTreeMap<i32, ShutdownTrigger>>>,
    tx: Sender<SourceMessage<OwnedMessage>>,
}

impl Source for KafkaTopicSource {
    type Result = OwnedMessage;

    fn subscribe(&self) -> Receiver<SourceMessage<Self::Result>> {
        self.tx.subscribe()
    }

    fn source_id(&self) -> &SourceId {
        &self.topic
    }
}

#[allow(clippy::too_many_arguments)]
/// Creates new consumers for partitions that have not yet been observed.
/// Each consumer will start consuming from the partition's high watermark.
async fn reconcile_partition_consumers<T: Into<Timeout> + Clone + Send + Sync + 'static>(
    topic: &str,
    client: Arc<Mutex<Client>>,
    client_config: &ClientConfig,
    tx: Sender<SourceMessage<OwnedMessage>>,
    metadata_fetch_timeout: T,
    watermarks_fetch_timeout: T,
    consumer_tasks: &Mutex<BTreeMap<i32, ShutdownTrigger>>,
    emit_metadata_changed: bool,
) -> anyhow::Result<()> {
    let metadata_client = Arc::clone(&client);
    let timeout = metadata_fetch_timeout.clone();
    let t = topic.to_string();

    let metadata = tokio::task::spawn_blocking(move || {
        let metadata_client = metadata_client.lock().expect("poisoned lock");

        metadata_client
            .fetch_metadata(Some(&t), timeout)
            .expect("Failed to fetch topic metadata")
    })
    .await?;

    assert_eq!(metadata.topics().len(), 1);

    let partitions = {
        let topic_metadata = metadata.topics().first().unwrap();
        topic_metadata
            .partitions()
            .iter()
            .map(|p| p.id())
            .collect::<Vec<_>>()
    };

    for partition in partitions {
        let watermarks_client = Arc::clone(&client);
        let t = topic.to_string();
        let watermarks_fetch_timeout = watermarks_fetch_timeout.clone();
        let (_, high) = tokio::task::spawn_blocking(move || {
            let watermarks_client = watermarks_client.lock().expect("poisoned lock");

            watermarks_client
                .fetch_watermarks(&t, partition, watermarks_fetch_timeout)
                .expect("Failed to fetch watermarks")
        })
        .await?;

        let mut consumer_tasks = consumer_tasks.lock().expect("poisoned lock");

        match consumer_tasks.entry(partition) {
            std::collections::btree_map::Entry::Occupied(_) => {
                // We already have a consumer task for this partition
                continue;
            }
            std::collections::btree_map::Entry::Vacant(entry) => {
                // We haven't seen this partition before, so we need to create
                // a new consumer task for it
                if emit_metadata_changed {
                    let _ = tx.send(SourceMessage::MetadataChanged(format!(
                        "New partition ({}) observed for topic {}",
                        topic, partition
                    )));
                }

                let (shutdown_trigger, shutdown_rx) = oneshot::channel::<()>();

                let partition_consumer = PartitionConsumer::new(
                    topic,
                    partition,
                    rdkafka::Offset::Offset(high),
                    client_config,
                    shutdown_rx.fuse(),
                    tx.clone(),
                )
                .context(format!(
                    "Failed to create partition consumer for topic/partition {}/{}",
                    topic, partition
                ))?;

                tokio::task::spawn(partition_consumer.run());

                tracing::debug!(
                    topic = topic,
                    partition = partition,
                    "Created new partition consumer",
                );

                entry.insert(shutdown_trigger);
            }
        }
    }

    Ok(())
}

impl KafkaTopicSource {
    pub async fn new(topic: String, client_config: &ClientConfig) -> anyhow::Result<Self> {
        // TODO: make this capacity configurable
        let (tx, _) = tokio::sync::broadcast::channel::<SourceMessage<OwnedMessage>>(100);
        let consumer_tasks = Arc::new(Mutex::new(BTreeMap::new()));
        // Transient consumer used to fetch metadata and watermarks

        let native_config = client_config.create_native_config()?;

        // Kafka only provides producer and consumer clients. We use a producer
        // for querying metadata and watermarks as they are allegedly more lightweight
        let client = Arc::new(Mutex::new(rdkafka::client::Client::new(
            client_config,
            native_config,
            rdkafka::types::RDKafkaType::RD_KAFKA_PRODUCER,
            DefaultClientContext,
        )?));

        reconcile_partition_consumers(
            topic.as_str(),
            Arc::clone(&client),
            client_config,
            tx.clone(),
            Duration::from_millis(5000),
            Duration::from_millis(5000),
            &consumer_tasks,
            false,
        )
        .await
        .context("Failed to initialize partition consumers")?;

        // TODO: Create a metadata refresh task that notifies
        // along tx when the topic metadata changes

        let weak_tasks = Arc::downgrade(&consumer_tasks);

        let result = Self {
            topic: topic.clone(),
            _partition_consumers: consumer_tasks,
            tx: tx.clone(),
        };

        let client_config = client_config.clone();

        tokio::task::spawn(async move {
            loop {
                if let Some(tasks) = weak_tasks.upgrade() {
                    if let Err(err) = reconcile_partition_consumers(
                        topic.as_str(),
                        Arc::clone(&client),
                        &client_config,
                        tx.clone(),
                        Duration::from_millis(5000),
                        Duration::from_millis(5000),
                        &tasks,
                        true,
                    )
                    .await
                    {
                        tracing::error!(
                            "Failed to reconcile partition consumers for topic {}: {}",
                            topic,
                            err
                        );
                    }
                } else {
                    // The absence of the consumer map indicates that the source
                    // has been dropped, so we can safely exit the thread
                    break;
                }

                // TODO: Make this similar to the configured metadata refresh interval
                tokio::time::sleep(Duration::from_millis(2000)).await;
            }
        });

        Ok(result)
    }
}

impl From<OwnedMessage> for hook::intercept::types::EventCtx {
    fn from(value: OwnedMessage) -> Self {
        Self::Kafka(value.into())
    }
}

impl From<OwnedMessage> for hook::intercept::types::KafkaEventCtx {
    fn from(value: OwnedMessage) -> Self {
        Self {
            payload: value.payload().map(|p| p.to_owned()),
            topic: value.topic().to_string(),
            timestamp: value.timestamp().to_millis(),
            partition: value.partition(),
            offset: value.offset(),
        }
    }
}

impl MutableEvent for OwnedMessage {
    fn set_payload(self, payload: Option<Vec<u8>>) -> Self {
        OwnedMessage::set_payload(self, payload)
    }
}

pub async fn build_sources(
    topics: impl Iterator<Item = String>,
    group_prefix: String,
    bootstrap_servers: Vec<String>,
) -> BTreeMap<SourceId, KafkaTopicSource> {
    let mut client_config = ClientConfig::new();
    let group_id = format!("{}{}", group_prefix, nanoid::nanoid!());
    let bootstrap_servers = bootstrap_servers.join(",");

    client_config.extend(btreemap! {
        "group.id".to_string() => group_id.to_string(),
        // We don't care about offset committing, since we are just relaying the latest messages.
        "enable.auto.commit".to_string() => "false".to_string(),
        "enable.partition.eof".to_string() => "false".to_string(),
        // A friendly label to present to Kafka
        "client.id".to_string() => "kiwi".to_string(),
        "bootstrap.servers".to_string() => bootstrap_servers,
        // TODO: make this configurable
        // "topic.metadata.refresh.interval.ms".to_string() => 2000.to_string(),
    });

    let mut result = BTreeMap::new();

    for topic in topics {
        let src = KafkaTopicSource::new(topic, &client_config)
            .await
            .expect("Failed to create Kafka topic source");

        result.insert(src.source_id().clone(), src);
    }

    result
}
