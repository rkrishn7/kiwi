use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Context;
use futures::stream::StreamExt;
use futures::{future::Fuse, FutureExt};
use maplit::btreemap;
use rdkafka::client::{Client, DefaultClientContext};
use rdkafka::{
    consumer::{Consumer, StreamConsumer},
    ClientConfig,
};
use rdkafka::{Message, TopicPartitionList};
use tokio::sync::{
    broadcast::{Receiver, Sender},
    oneshot,
};

use crate::hook;

use super::{Source, SourceId, SourceMessage, SourceMetadata, SourceResult, SubscribeError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KafkaSourceResult {
    /// Source ID
    pub id: SourceId,
    /// Event key
    pub key: Option<Vec<u8>>,
    /// Event payload
    pub payload: Option<Vec<u8>>,
    /// Topic this event was produced from
    pub topic: String,
    /// Timestamp at which the message was produced
    pub timestamp: Option<i64>,
    /// Partition ID this event was produced from
    pub partition: i32,
    /// Offset at which the message was produced
    pub offset: i64,
}

#[derive(Debug, Clone)]
pub struct KafkaSourceMetadata {
    partitions: Vec<PartitionMetadata>,
}

pub struct PartitionConsumer {
    source_id: SourceId,
    consumer: StreamConsumer,
    shutdown_rx: Fuse<oneshot::Receiver<()>>,
    tx: Sender<SourceMessage>,
}

impl PartitionConsumer {
    pub fn new<'a>(
        source_id: SourceId,
        topic: &'a str,
        partition: i32,
        offset: rdkafka::Offset,
        client_config: &'a ClientConfig,
        shutdown_rx: Fuse<oneshot::Receiver<()>>,
        tx: Sender<SourceMessage>,
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
            source_id,
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
                                    let owned_message = borrowed_message.detach();
                                    // An error here does not mean future calls will fail, since new subscribers
                                    // may be created. If there are no subscribers, we simply discard the message
                                    // and move on
                                    let _ = self.tx.send(SourceMessage::Result(SourceResult::Kafka(KafkaSourceResult {
                                        id: self.source_id.clone(),
                                        key: owned_message.key().map(|k| k.to_owned()),
                                        payload: owned_message.payload().map(|p| p.to_owned()),
                                        topic: owned_message.topic().to_string(),
                                        timestamp: owned_message.timestamp().to_millis(),
                                        partition: owned_message.partition(),
                                        offset: owned_message.offset(),
                                    })));
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
    id: SourceId,
    topic: String,
    // Map of partition ID -> shutdown trigger
    _partition_consumers: Arc<Mutex<BTreeMap<i32, ShutdownTrigger>>>,
    tx: Sender<SourceMessage>,
    metadata_tx: Option<tokio::sync::mpsc::UnboundedSender<SourceMetadata>>,
}

impl Source for KafkaTopicSource {
    fn subscribe(&mut self) -> Result<Receiver<SourceMessage>, SubscribeError> {
        Ok(self.tx.subscribe())
    }

    fn source_id(&self) -> &SourceId {
        &self.id
    }

    fn metadata_tx(&self) -> &Option<tokio::sync::mpsc::UnboundedSender<SourceMetadata>> {
        &self.metadata_tx
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl KafkaTopicSource {
    pub fn new(
        id: SourceId,
        topic: String,
        bootstrap_servers: &[String],
        group_id_prefix: &str,
    ) -> anyhow::Result<Self> {
        // TODO: make this capacity configurable
        let (tx, _) = tokio::sync::broadcast::channel::<SourceMessage>(100);
        let (metadata_tx, mut metadata_rx) =
            tokio::sync::mpsc::unbounded_channel::<SourceMetadata>();
        let consumer_tasks = Arc::new(Mutex::new(BTreeMap::new()));

        // Transient client used to fetch metadata and watermarks
        let metadata_client = create_metadata_client(bootstrap_servers)?;

        let mut client_config = ClientConfig::new();

        let group_id = format!("{}{}", group_id_prefix, nanoid::nanoid!());

        client_config.extend(btreemap! {
            "group.id".to_string() => group_id,
            // We don't care about offset committing, since we are just relaying the latest messages.
            "enable.auto.commit".to_string() => "true".to_string(),
            "enable.partition.eof".to_string() => "false".to_string(),
            // A friendly label to present to Kafka
            "client.id".to_string() => "kiwi".to_string(),
            "bootstrap.servers".to_string() => bootstrap_servers.join(","),
            "topic.metadata.refresh.interval.ms".to_string() => (-1).to_string(),
        });

        for partition_metadata in fetch_partition_metadata(topic.as_str(), &metadata_client)? {
            let (shutdown_trigger, shutdown_rx) = oneshot::channel::<()>();

            let partition_consumer = PartitionConsumer::new(
                id.clone(),
                topic.as_str(),
                partition_metadata.partition,
                rdkafka::Offset::Offset(partition_metadata.hi_watermark),
                &client_config,
                shutdown_rx.fuse(),
                tx.clone(),
            )
            .context(format!(
                "Failed to create partition consumer for topic/partition {}/{}",
                topic, partition_metadata.partition
            ))?;

            tokio::task::spawn(partition_consumer.run());

            consumer_tasks
                .lock()
                .expect("poisoned lock")
                .insert(partition_metadata.partition, shutdown_trigger);
        }

        let weak_tasks = Arc::downgrade(&consumer_tasks);

        let result = Self {
            id: id.clone(),
            topic: topic.clone(),
            _partition_consumers: consumer_tasks,
            tx: tx.clone(),
            metadata_tx: Some(metadata_tx),
        };

        let client_config = client_config.clone();

        tokio::task::spawn(async move {
            while let Some(metadata) = metadata_rx.recv().await {
                if let Some(tasks) = weak_tasks.upgrade() {
                    match metadata {
                        SourceMetadata::Kafka(topic_metadata) => {
                            for PartitionMetadata {
                                partition,
                                hi_watermark,
                                ..
                            } in topic_metadata.partitions
                            {
                                let mut tasks = tasks.lock().expect("poisoned lock");

                                match tasks.entry(partition) {
                                    std::collections::btree_map::Entry::Vacant(entry) => {
                                        let (shutdown_trigger, shutdown_rx) =
                                            oneshot::channel::<()>();

                                        match PartitionConsumer::new(
                                            id.clone(),
                                            topic.as_str(),
                                            partition,
                                            rdkafka::Offset::Offset(hi_watermark),
                                            &client_config,
                                            shutdown_rx.fuse(),
                                            tx.clone(),
                                        ) {
                                            Ok(partition_consumer) => {
                                                let _ = tx.send(SourceMessage::MetadataChanged(
                                                    format!(
                                                        "New partition ({}) observed for topic {}",
                                                        topic, partition
                                                    ),
                                                ));

                                                tokio::task::spawn(partition_consumer.run());
                                                entry.insert(shutdown_trigger);

                                                tracing::debug!(
                                                    topic = topic.as_str(),
                                                    partition = partition,
                                                    "Observed new partition. Created new partition consumer"
                                                );
                                            }
                                            Err(err) => {
                                                tracing::error!(
                                                    topic = topic,
                                                    partition = partition,
                                                    error = ?err,
                                                    "Failed to create partition consumer",
                                                );
                                            }
                                        }
                                    }
                                    std::collections::btree_map::Entry::Occupied(_) => (),
                                }
                            }
                        }
                    }
                } else {
                    tracing::debug!(
                        topic = topic.as_str(),
                        "Topic source has been dropped. Exiting partition creation task"
                    );
                    // The absence of the consumer map indicates that the source
                    // has been dropped, so we can safely exit the task
                    break;
                }
            }
        });

        Ok(result)
    }
}

#[derive(Debug, Clone)]
pub struct PartitionMetadata {
    pub partition: i32,
    pub hi_watermark: i64,
    pub lo_watermark: i64,
}

fn create_metadata_client(bootstrap_servers: &[String]) -> anyhow::Result<Client> {
    let mut client_config = ClientConfig::new();

    client_config.extend(btreemap! {
        "client.id".to_string() => "kiwi-metadata".to_string(),
        "bootstrap.servers".to_string() => bootstrap_servers.join(","),
    });

    let native_config = client_config.create_native_config()?;

    // Kafka only provides producer and consumer clients. We use a producer
    // for querying metadata and watermarks as they are allegedly more lightweight
    let client = rdkafka::client::Client::new(
        &client_config,
        native_config,
        rdkafka::types::RDKafkaType::RD_KAFKA_PRODUCER,
        DefaultClientContext,
    )?;

    Ok(client)
}

fn fetch_partition_metadata(
    topic: &str,
    client: &Client,
) -> anyhow::Result<Vec<PartitionMetadata>> {
    let mut result = Vec::new();

    let metadata = client.fetch_metadata(Some(topic), Duration::from_millis(5000))?;
    let topic = &metadata.topics()[0];

    for partition in topic.partitions() {
        let (low, hi) =
            client.fetch_watermarks(topic.name(), partition.id(), Duration::from_millis(5000))?;
        result.push(PartitionMetadata {
            partition: partition.id(),
            hi_watermark: hi,
            lo_watermark: low,
        });
    }

    Ok(result)
}

pub fn start_partition_discovery(
    bootstrap_servers: &[String],
    sources: Arc<Mutex<BTreeMap<SourceId, Box<dyn Source + Send + Sync + 'static>>>>,
    poll_interval: Duration,
) -> anyhow::Result<()> {
    let client = create_metadata_client(bootstrap_servers)?;

    std::thread::spawn(move || loop {
        std::thread::sleep(poll_interval);

        let kafka_sources = sources
            .lock()
            .expect("poisoned lock")
            .iter()
            .filter_map(|(_, source)| {
                source
                    .as_any()
                    .downcast_ref::<KafkaTopicSource>()
                    .map(|source| (source.id.clone(), source.topic.clone()))
            })
            .collect::<Vec<_>>();

        for (id, topic) in kafka_sources.iter() {
            match fetch_partition_metadata(topic.as_str(), &client) {
                Ok(metadata) => {
                    if let Some(source) = sources.lock().expect("poisoned lock").get(id) {
                        for partition_metadata in metadata {
                            let metadata_tx = source.metadata_tx();

                            if let Some(metadata_tx) = metadata_tx {
                                let _ =
                                    metadata_tx.send(SourceMetadata::Kafka(KafkaSourceMetadata {
                                        partitions: vec![partition_metadata],
                                    }));
                            }
                        }
                    }
                }
                Err(err) => {
                    tracing::error!(topic = topic.as_str(), error = ?err, "Failed to fetch partition metadata for topic");
                }
            }
        }
    });

    Ok(())
}

pub trait KafkaSourceBuilder {
    fn build_source(
        id: SourceId,
        topic: String,
        bootstrap_servers: &[String],
        group_id_prefix: &str,
    ) -> anyhow::Result<Box<dyn Source + Send + Sync + 'static>> {
        Ok(Box::new(KafkaTopicSource::new(
            id,
            topic,
            bootstrap_servers,
            group_id_prefix,
        )?))
    }
}

impl From<KafkaSourceResult> for hook::intercept::types::KafkaEventCtx {
    fn from(value: KafkaSourceResult) -> Self {
        Self {
            payload: value.payload,
            topic: value.topic,
            timestamp: value.timestamp,
            partition: value.partition,
            offset: value.offset,
        }
    }
}
