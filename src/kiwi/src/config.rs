use std::{
    collections::{BTreeMap, HashSet},
    fs::File,
    io::Read,
    path::Path,
    sync::{Arc, Mutex},
};

use anyhow::Context;
use notify::{Event, RecommendedWatcher, Watcher};
use serde::Deserialize;

use crate::source::counter::build_source as build_counter_source;
use crate::source::kafka::build_source as build_kafka_source;
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

pub fn reconcile_sources(
    sources: &mut BTreeMap<SourceId, Box<dyn Source + Send + Sync>>,
    config: &Config,
) -> anyhow::Result<()> {
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
                            build_kafka_source(
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
                    } => build_counter_source(
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_hooks() {
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
    fn test_sources_required() {
        let config = "
        server:
            address: '127.0.0.1:8000'
        ";

        assert!(Config::from_str(config).is_err());
    }

    #[test]
    fn test_kafka_sources() {
        // Ensure we can parse a config that includes kafka sources
        let config = "
        sources:
            - type: kafka
              topic: test
        server:
            address: '127.0.0.1:8000'
        ";

        let config = Config::from_str(config).unwrap();

        assert!(config.sources.len() == 1);
        assert!(
            matches!(config.sources[0].clone(), SourceType::Kafka { topic } if topic == "test")
        );
    }

    #[test]
    fn test_kafka_config() {
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
}
