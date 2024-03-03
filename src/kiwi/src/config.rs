use std::{
    collections::{BTreeMap, HashSet},
    fs::File,
    io::Read,
    path::Path,
    sync::{Arc, Mutex},
};

use anyhow::Context;
use serde::Deserialize;

use crate::source::SourceBuilder;
use crate::source::{counter::CounterSourceBuilder, kafka::KafkaSourceBuilder};
use crate::source::{Source, SourceId};

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub sources: Vec<SourceType>,
    pub hooks: Option<Hooks>,
    pub server: Server,
    pub kafka: Option<Kafka>,
    #[serde(default)]
    pub subscriber: Subscriber,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
#[serde(rename_all = "lowercase")]
pub enum SourceType {
    Kafka {
        topic: String,
    },
    Counter {
        id: String,
        min: u64,
        #[serde(default)]
        max: Option<u64>,
        interval_ms: u64,
        #[serde(default)]
        lazy: bool,
    },
}

#[derive(Debug, Clone, Deserialize)]
pub struct Kafka {
    #[serde(default = "Kafka::default_group_prefix")]
    pub group_id_prefix: String,
    pub bootstrap_servers: Vec<String>,
    #[serde(default = "Kafka::default_partition_discovery_enabled")]
    pub partition_discovery_enabled: bool,
    #[serde(default = "Kafka::default_partition_discovery_interval_ms")]
    pub partition_discovery_interval_ms: u32,
}

impl Kafka {
    fn default_group_prefix() -> String {
        "kiwi-".into()
    }

    fn default_partition_discovery_enabled() -> bool {
        true
    }

    fn default_partition_discovery_interval_ms() -> u32 {
        300000
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Hooks {
    pub intercept: Option<String>,
    pub authenticate: Option<String>,
    pub __adapter_path: Option<String>,
}

/// Server configuration
#[derive(Debug, Clone, Deserialize)]
pub struct Server {
    pub address: String,
}

/// General subscriber configuration
#[derive(Debug, Default, Clone, Deserialize)]
pub struct Subscriber {
    #[serde(default)]
    pub buffer_capacity: Option<usize>,
    #[serde(default)]
    pub lag_notice_threshold: Option<u64>,
}

impl Config {
    pub fn parse<P: AsRef<Path>>(path: P) -> Result<Self, anyhow::Error> {
        let mut file = File::open(path).context("failed to open kiwi config")?;
        let mut contents = String::new();
        file.read_to_string(&mut contents)?;

        Self::from_str(&contents)
    }

    fn from_str(contents: &str) -> Result<Self, anyhow::Error> {
        let config = serde_yaml::from_str::<'_, Config>(contents)?;

        Ok(config)
    }
}

pub struct ConfigReconciler<B: KafkaSourceBuilder + CounterSourceBuilder = SourceBuilder> {
    sources: Arc<Mutex<BTreeMap<SourceId, Box<dyn Source + Send + Sync>>>>,
    _builder: std::marker::PhantomData<B>,
}

impl<B: KafkaSourceBuilder + CounterSourceBuilder> ConfigReconciler<B> {
    pub fn new(sources: Arc<Mutex<BTreeMap<SourceId, Box<dyn Source + Send + Sync>>>>) -> Self {
        Self {
            sources,
            _builder: std::marker::PhantomData,
        }
    }

    pub fn reconcile_sources(&self, config: &Config) -> anyhow::Result<()> {
        let mut sources = self.sources.lock().expect("poisoned lock");
        let mut seen = HashSet::new();

        for typ in config.sources.iter() {
            let incoming = match typ {
                SourceType::Kafka { topic } => topic,
                SourceType::Counter { id, .. } => id,
            };

            if !seen.insert(incoming) {
                return Err(anyhow::anyhow!(
                    "Found duplicate source ID in configuration: {}",
                    incoming
                ));
            }

            match sources.entry(incoming.clone()) {
                std::collections::btree_map::Entry::Occupied(_) => {
                    // Source already exists
                    continue;
                }
                std::collections::btree_map::Entry::Vacant(entry) => {
                    // Build and add source
                    let source = match typ {
                        SourceType::Kafka { topic } => {
                            if let Some(kafka_config) = config.kafka.as_ref() {
                                <B as KafkaSourceBuilder>::build_source(
                                    topic.clone(),
                                    &kafka_config.bootstrap_servers,
                                    &kafka_config.group_id_prefix,
                                )?
                            } else {
                                return Err(anyhow::anyhow!(
                                    "Kafka source specified but no Kafka configuration found"
                                ));
                            }
                        }
                        SourceType::Counter {
                            id,
                            min,
                            max,
                            interval_ms,
                            lazy,
                        } => <B as CounterSourceBuilder>::build_source(
                            id.clone(),
                            *min,
                            *max,
                            std::time::Duration::from_millis(*interval_ms),
                            *lazy,
                        ),
                    };

                    tracing::info!("Built source from configuration: {}", source.source_id());
                    entry.insert(source);
                }
            };
        }

        sources.retain(|id, _| {
            if !config.sources.iter().any(|typ| match typ {
                SourceType::Kafka { topic } => topic == id,
                SourceType::Counter { id: incoming, .. } => incoming == id,
            }) {
                tracing::info!("Removing source due to configuration change: {}", id);
                false
            } else {
                true
            }
        });

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    #[test]
    fn test_parses_hooks() {
        // Ensure we can parse a config that includes hooks
        let config = "
        hooks:
            intercept: ./intercept.wasm
            authenticate: ./auth.wasm
        sources:
            - type: kafka
              topic: test
        server:
            address: '127.0.0.1:8000'
        ";

        let config = Config::from_str(config).unwrap();

        assert!(config.hooks.is_some());
        assert_eq!(
            config.hooks.clone().unwrap().intercept,
            Some("./intercept.wasm".into())
        );
        assert_eq!(
            config.hooks.unwrap().authenticate,
            Some("./auth.wasm".into())
        );

        // Ensure we can parse a config that does not include any plugins
        let config = "
        sources:
            - type: kafka
              topic: test
        server:
            address: '127.0.0.1:8000'
        ";

        let config = Config::from_str(config).unwrap();

        assert!(config.hooks.is_none());
    }

    #[test]
    fn test_requires_sources() {
        let config = "
        server:
            address: '127.0.0.1:8000'
        ";

        assert!(Config::from_str(config).is_err());
    }

    #[test]
    fn test_parses_sources() {
        let config = "
        sources:
            - type: kafka
              topic: test
            - type: counter
              id: test
              min: 0
              interval_ms: 100
        server:
            address: '127.0.0.1:8000'
        ";

        let config = Config::from_str(config).unwrap();

        assert!(config.sources.len() == 2);
        assert!(
            matches!(config.sources[0].clone(), SourceType::Kafka { topic } if topic == "test")
        );

        assert!(matches!(
            config.sources[1].clone(),
            SourceType::Counter {
                id,
                min,
                interval_ms,
                lazy,
                ..
            } if id == "test" && min == 0 && interval_ms == 100 && !lazy
        ));

        let config = "
        sources:
            - type: counter
              id: test
              min: 0
              interval_ms: 100
              lazy: true
              max: 100
        server:
            address: '127.0.0.1:8000'
        ";

        let config = Config::from_str(config).unwrap();

        assert!(matches!(
            config.sources[0].clone(),
            SourceType::Counter {
                id,
                min,
                interval_ms,
                lazy,
                max,
            } if id == "test" && min == 0 && interval_ms == 100 && lazy && max == Some(100)
        ));
    }

    #[test]
    fn test_parses_kafka_config() {
        // Test default values
        let config = "
        sources: []
        server:
            address: '127.0.0.1:8000'
        kafka:
            bootstrap_servers:
                - 'localhost:9092'
        ";

        let config = Config::from_str(config).unwrap();

        assert!(config.kafka.is_some());
        assert!(config.kafka.as_ref().unwrap().partition_discovery_enabled);
        assert_eq!(
            config
                .kafka
                .as_ref()
                .unwrap()
                .partition_discovery_interval_ms,
            300000
        );
        assert!(config.kafka.as_ref().unwrap().group_id_prefix == "kiwi-");
        assert!(config.kafka.as_ref().unwrap().bootstrap_servers.len() == 1);
        assert!(config.kafka.as_ref().unwrap().bootstrap_servers[0] == "localhost:9092");
    }

    struct TestSource(String);

    impl TestSource {
        fn new(id: &str) -> Self {
            Self(id.into())
        }
    }

    impl Source for TestSource {
        fn source_id(&self) -> &SourceId {
            &self.0
        }

        fn subscribe(
            &mut self,
        ) -> Result<
            tokio::sync::broadcast::Receiver<crate::source::SourceMessage>,
            crate::source::SubscribeError,
        > {
            unimplemented!()
        }

        fn metadata_tx(
            &self,
        ) -> &Option<tokio::sync::mpsc::UnboundedSender<crate::source::SourceMetadata>> {
            &None
        }
    }

    struct TestSourceBuilder;

    impl CounterSourceBuilder for TestSourceBuilder {
        fn build_source(
            id: SourceId,
            _min: u64,
            _max: Option<u64>,
            _interval: std::time::Duration,
            _lazy: bool,
        ) -> Box<dyn Source + Send + Sync> {
            Box::new(TestSource::new(&id))
        }
    }

    impl KafkaSourceBuilder for TestSourceBuilder {
        fn build_source(
            topic: SourceId,
            _bootstrap_servers: &[String],
            _group_id_prefix: &str,
        ) -> Result<Box<dyn Source + Send + Sync>, anyhow::Error> {
            Ok(Box::new(TestSource::new(&topic)))
        }
    }

    #[test]
    fn test_reconciliation_duplicate_sources_in_config() {
        let config_reconciler: ConfigReconciler<TestSourceBuilder> =
            ConfigReconciler::new(Arc::new(Mutex::new(BTreeMap::new())));

        let config = Config {
            sources: vec![
                SourceType::Counter {
                    id: "test".into(),
                    min: 0,
                    max: None,
                    interval_ms: 100,
                    lazy: false,
                },
                SourceType::Kafka {
                    topic: "test".into(),
                },
            ],
            hooks: None,
            server: Server {
                address: "127.0.0.1:8000".into(),
            },
            kafka: None,
            subscriber: Subscriber::default(),
        };

        assert!(config_reconciler.reconcile_sources(&config).is_err());
    }

    #[test]
    fn test_reconciliation_requires_kafka_config_if_kafka_source_present() {
        let config_reconciler: ConfigReconciler<TestSourceBuilder> =
            ConfigReconciler::new(Arc::new(Mutex::new(BTreeMap::new())));

        let config = Config {
            sources: vec![SourceType::Kafka {
                topic: "test".into(),
            }],
            hooks: None,
            server: Server {
                address: "127.0.0.1:8000".into(),
            },
            kafka: None,
            subscriber: Subscriber::default(),
        };

        assert!(config_reconciler.reconcile_sources(&config).is_err());

        let config = Config {
            sources: vec![SourceType::Kafka {
                topic: "test".into(),
            }],
            hooks: None,
            server: Server {
                address: "127.0.0.1:8000".into(),
            },
            kafka: Some(Kafka {
                group_id_prefix: "kiwi-".into(),
                bootstrap_servers: vec!["localhost:9092".into()],
                partition_discovery_enabled: true,
                partition_discovery_interval_ms: 300000,
            }),
            subscriber: Subscriber::default(),
        };

        assert!(config_reconciler.reconcile_sources(&config).is_ok());
    }

    #[test]
    fn test_reconciliation_adds_counter_source() {
        let sources = Arc::new(Mutex::new(BTreeMap::new()));
        let config_reconciler: ConfigReconciler<TestSourceBuilder> =
            ConfigReconciler::new(Arc::clone(&sources));

        let config = Config {
            sources: vec![SourceType::Counter {
                id: "test".into(),
                min: 0,
                max: None,
                interval_ms: 100,
                lazy: false,
            }],
            hooks: None,
            server: Server {
                address: "127.0.0.1:8000".into(),
            },
            kafka: None,
            subscriber: Subscriber::default(),
        };

        assert!(config_reconciler.reconcile_sources(&config).is_ok());

        let sources = sources.lock().unwrap();
        assert!(sources.len() == 1);
        assert!(sources.contains_key("test"));
    }

    #[test]
    fn test_reconciliation_removes_sources_not_in_config() {
        let sources = Arc::new(Mutex::new(BTreeMap::new()));

        sources.lock().unwrap().insert(
            "test".into(),
            <TestSourceBuilder as CounterSourceBuilder>::build_source(
                "test".into(),
                0,
                None,
                Duration::from_millis(100),
                false,
            ),
        );

        sources.lock().unwrap().insert(
            "topic1".into(),
            <TestSourceBuilder as KafkaSourceBuilder>::build_source(
                "topic1".into(),
                &["localhost:9092".into()],
                "kiwi-",
            )
            .unwrap(),
        );

        let config_reconciler: ConfigReconciler<TestSourceBuilder> =
            ConfigReconciler::new(Arc::clone(&sources));

        let config = Config {
            sources: vec![],
            hooks: None,
            server: Server {
                address: "127.0.0.1:8000".into(),
            },
            kafka: None,
            subscriber: Subscriber::default(),
        };

        assert!(config_reconciler.reconcile_sources(&config).is_ok());

        let sources = sources.lock().unwrap();
        assert!(sources.len() == 0);
    }

    #[test]
    fn test_reconciliation_retains_existing_sources_present_in_config() {
        let sources = Arc::new(Mutex::new(BTreeMap::new()));

        sources.lock().unwrap().insert(
            "test".into(),
            <TestSourceBuilder as CounterSourceBuilder>::build_source(
                "test".into(),
                0,
                None,
                Duration::from_millis(100),
                false,
            ),
        );

        sources.lock().unwrap().insert(
            "topic1".into(),
            <TestSourceBuilder as KafkaSourceBuilder>::build_source(
                "topic1".into(),
                &["localhost:9092".into()],
                "kiwi-",
            )
            .unwrap(),
        );

        let config_reconciler: ConfigReconciler<TestSourceBuilder> =
            ConfigReconciler::new(Arc::clone(&sources));

        let config = Config {
            sources: vec![SourceType::Counter {
                id: "test".into(),
                min: 0,
                max: None,
                interval_ms: 100,
                lazy: false,
            }],
            hooks: None,
            server: Server {
                address: "127.0.0.1:8000".into(),
            },
            kafka: None,
            subscriber: Subscriber::default(),
        };

        assert!(config_reconciler.reconcile_sources(&config).is_ok());

        let sources = sources.lock().unwrap();
        assert!(sources.len() == 1);
        assert!(sources.contains_key("test"));
    }
}
