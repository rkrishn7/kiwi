use std::collections::{BTreeMap, BTreeSet};

use maplit::btreemap;
use rdkafka::admin::{AdminClient as RdKafkaAdminClient, AdminOptions, NewPartitions};
use rdkafka::client::DefaultClientContext;
use rdkafka::config::ClientConfig;
use rdkafka::producer::{FutureProducer, FutureRecord};

type KafkaAdminClient = RdKafkaAdminClient<DefaultClientContext>;

pub struct AdminClient {
    inner: KafkaAdminClient,
    options: AdminOptions,
    topics: BTreeSet<String>,
}

impl AdminClient {
    pub fn new(bootstrap_servers: &str) -> anyhow::Result<Self> {
        Self::new_with_config(btreemap! {
            "bootstrap.servers" => bootstrap_servers,
        })
    }

    pub fn new_with_config<K, V>(config: BTreeMap<K, V>) -> anyhow::Result<Self>
    where
        K: Into<String>,
        V: Into<String>,
    {
        let mut client_config = ClientConfig::new();

        client_config.extend(config.into_iter().map(|(k, v)| (k.into(), v.into())));

        let admin_client = client_config.create::<KafkaAdminClient>()?;

        Ok(Self {
            inner: admin_client,
            options: Default::default(),
            topics: Default::default(),
        })
    }

    pub async fn create_random_topic(&mut self, num_partitions: i32) -> anyhow::Result<String> {
        let topic = format!("test-{}", nanoid::nanoid!());

        self.create_topic(&topic, num_partitions, 1).await?;

        Ok(topic)
    }

    #[allow(dead_code)]
    pub async fn create_named_topic(
        &mut self,
        topic: &str,
        num_partitions: i32,
    ) -> anyhow::Result<()> {
        self.create_topic(topic, num_partitions, 1).await
    }

    pub async fn update_partitions(
        &mut self,
        topic_name: &str,
        new_partition_count: usize,
    ) -> anyhow::Result<()> {
        let result = self
            .inner
            .create_partitions(
                &[NewPartitions {
                    topic_name,
                    new_partition_count,
                    assignment: None,
                }],
                &self.options,
            )
            .await?;

        result[0].as_ref().map_err(|(topic, error)| {
            anyhow::anyhow!("Failed to add partitions to topic {}: {}", topic, error)
        })?;

        Ok(())
    }

    async fn create_topic(
        &mut self,
        topic: &str,
        num_partitions: i32,
        replication_factor: i32,
    ) -> anyhow::Result<()> {
        let new_topic = rdkafka::admin::NewTopic::new(
            topic,
            num_partitions,
            rdkafka::admin::TopicReplication::Fixed(replication_factor),
        );

        let result = self
            .inner
            .create_topics(&[new_topic], &self.options)
            .await?;

        result[0].as_ref().map_err(|(topic, error)| {
            anyhow::anyhow!("Failed to create topic {}: {}", topic, error)
        })?;

        assert!(self.topics.insert(topic.to_string()));

        Ok(())
    }

    async fn delete_topics(&mut self, topics: &[&str]) -> anyhow::Result<()> {
        let result = self.inner.delete_topics(topics, &self.options).await?;

        for result in result {
            match result {
                Ok(topic) => {
                    self.topics.remove(&topic);
                }
                Err((topic, error)) => {
                    return Err(anyhow::anyhow!(
                        "Failed to delete topic {}: {}",
                        topic,
                        error
                    ));
                }
            }
        }

        Ok(())
    }
}

impl Drop for AdminClient {
    fn drop(&mut self) {
        let topics = self.topics.clone();
        let topics: Vec<&str> = topics.iter().map(|s| s.as_ref()).collect();

        if !topics.is_empty() {
            futures::executor::block_on(async { self.delete_topics(topics.as_slice()).await })
                .expect("Failed to delete topics");
        }
    }
}

pub struct Producer {
    inner: FutureProducer,
}

impl Producer {
    pub fn new(bootstrap_servers: &str) -> anyhow::Result<Self> {
        let producer: FutureProducer = rdkafka::config::ClientConfig::new()
            .set("bootstrap.servers", bootstrap_servers)
            .set("message.timeout.ms", "5000")
            .create()?;

        Ok(Self { inner: producer })
    }

    pub async fn send(&self, topic: &str, key: &str, payload: &str) -> anyhow::Result<()> {
        let record = FutureRecord::to(topic).payload(payload).key(key);

        self.inner
            .send(record, std::time::Duration::from_secs(0))
            .await
            .map_err(|(e, _)| anyhow::anyhow!("Failed to send message: {}", e))?;

        Ok(())
    }
}
