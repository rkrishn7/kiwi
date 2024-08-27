use std::{
    collections::{BTreeMap, HashSet},
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use anyhow::Context;
use arc_swap::{access::Access, ArcSwapOption};
use notify::{RecommendedWatcher, Watcher};
use serde::Deserialize;

use crate::{
    hook::wasm::WasmAuthenticateHook,
    source::{counter::CounterSourceBuilder, kafka::KafkaSourceBuilder},
};
use crate::{
    hook::wasm::WasmHook,
    source::{Source, SourceId},
};
use crate::{hook::wasm::WasmInterceptHook, source::SourceBuilder};

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
        id: Option<SourceId>,
        topic: String,
    },
    Counter {
        id: SourceId,
        min: u64,
        #[serde(default)]
        max: Option<u64>,
        interval_ms: u64,
        #[serde(default)]
        lazy: bool,
    },
}

impl SourceType {
    pub fn id(&self) -> &SourceId {
        match self {
            SourceType::Kafka { id, topic } => id.as_ref().unwrap_or(topic),
            SourceType::Counter { id, .. } => id,
        }
    }
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
}

/// Server configuration
#[derive(Debug, Clone, Deserialize)]
pub struct Server {
    pub address: String,
    pub tls: Option<Tls>,
    #[serde(default = "Server::default_healthcheck_enabled")]
    pub healthcheck: bool,
}

impl Server {
    fn default_healthcheck_enabled() -> bool {
        true
    }
}

/// TLS configuration
#[derive(Debug, Clone, Deserialize)]
pub struct Tls {
    pub cert: PathBuf,
    pub key: PathBuf,
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

pub struct ConfigReconciler<A = WasmAuthenticateHook, B = SourceBuilder, I = WasmInterceptHook> {
    sources: Arc<Mutex<BTreeMap<SourceId, Box<dyn Source + Send + Sync>>>>,
    intercept: Arc<ArcSwapOption<I>>,
    authenticate: Arc<ArcSwapOption<A>>,
    _builder: std::marker::PhantomData<B>,
}

/// Adds a watch to the specified path after a delay.
/// Useful in cases where the file cannot be swapped atomically
async fn watch_path_with_delay(
    watcher: &mut RecommendedWatcher,
    path: &Path,
    delay: std::time::Duration,
) -> anyhow::Result<()> {
    tokio::time::sleep(delay).await;
    watcher.watch(path, notify::RecursiveMode::NonRecursive)?;
    Ok(())
}

/// Recompiles the specified hook from its cached file path and adapter path
fn reload_hook<T: WasmHook>(hook: &Arc<ArcSwapOption<T>>) -> anyhow::Result<()> {
    if let Some(module_path) = hook.load().as_ref().map(|h| h.path()) {
        hook.store(Some(Arc::new(T::from_file(module_path)?)));
        tracing::info!("Recompiled hook at {:?}", module_path);
    }

    Ok(())
}

/// Reconcile a hook from a file path. If the path is `None`, the hook is removed.
/// If the path is not `None`, the hook is recompiled if the path has changed.
fn reconcile_hook<T: WasmHook>(
    hook: &Arc<ArcSwapOption<T>>,
    module_path: Option<&String>,
) -> anyhow::Result<()> {
    if let Some(path) = module_path {
        if let Some(last_known_path) = hook.load().as_ref().map(|h| h.path()) {
            let updated_path: &Path = path.as_ref();

            // If the path has changed, recompile the hook
            //
            // TODO(rkrishn7): Currently, if the path is updated, the watch is not removed
            // on the old path. While unlikely the path will be updated frequently, it is
            // still something to clean up.
            if last_known_path != updated_path {
                hook.store(Some(Arc::new(T::from_file(path)?)));

                tracing::info!("Recompiled hook at {:?}", path);
            }
        } else {
            hook.store(Some(Arc::new(T::from_file(path)?)));

            tracing::info!("Compiled hook at {:?}", path);
        }
    } else if let Some(path) = hook.load().as_ref().map(|h| h.path()) {
        tracing::info!("Removing hook at {:?}", path);
        hook.store(None);
    }

    Ok(())
}

impl<A: WasmHook, B: KafkaSourceBuilder + CounterSourceBuilder, I: WasmHook>
    ConfigReconciler<A, B, I>
{
    pub fn new(
        sources: Arc<Mutex<BTreeMap<SourceId, Box<dyn Source + Send + Sync>>>>,
        intercept: Arc<ArcSwapOption<I>>,
        authenticate: Arc<ArcSwapOption<A>>,
    ) -> Self {
        Self {
            sources,
            intercept,
            authenticate,
            _builder: std::marker::PhantomData,
        }
    }

    pub async fn watch(self, conf_path: PathBuf) -> anyhow::Result<()> {
        let (tx, mut rx) = tokio::sync::mpsc::channel(1);

        let mut watcher = RecommendedWatcher::new(
            move |res: notify::Result<notify::Event>| {
                if let Ok(event) = res {
                    futures::executor::block_on(async {
                        tx.send(event).await.expect("rx dropped");
                    });
                }
            },
            notify::Config::default(),
        )?;

        // Setup initial watches
        watcher.watch(conf_path.as_path(), notify::RecursiveMode::NonRecursive)?;
        if let Some(intercept) = self.intercept.load().as_ref() {
            watcher.watch(intercept.path(), notify::RecursiveMode::NonRecursive)?;
        }
        if let Some(authenticate) = self.authenticate.load().as_ref() {
            watcher.watch(authenticate.path(), notify::RecursiveMode::NonRecursive)?;
        }

        fn contains_path(ev: &notify::Event, path: &Path) -> bool {
            ev.paths.iter().any(|p| p.ends_with(path))
        }

        while let Some(ev) = rx.recv().await {
            match ev.kind {
                notify::EventKind::Access(notify::event::AccessKind::Close(
                    notify::event::AccessMode::Write,
                ))
                | notify::EventKind::Remove(_) => {
                    // This event is related to the config file
                    if contains_path(&ev, &conf_path) {
                        if ev.kind.is_remove() {
                            // Add back the watch
                            watch_path_with_delay(
                                &mut watcher,
                                &conf_path,
                                std::time::Duration::from_millis(100),
                            )
                            .await?;
                        }

                        if let Err(e) = Config::parse(&conf_path)
                            .context("failed to parse config")
                            .and_then(|config| {
                                self.reconcile_sources(&config)?;
                                self.reconcile_hooks(&config)?;

                                Ok(())
                            })
                        {
                            tracing::error!("Failed to reconcile configuration update: {:?}", e);
                        } else {
                            tracing::info!("Successfully reconciled configuration update");
                        }
                    }
                    if let Some(intercept) = self.intercept.load().as_ref() {
                        if contains_path(&ev, intercept.path()) {
                            if ev.kind.is_remove() {
                                // Add back the watch
                                watch_path_with_delay(
                                    &mut watcher,
                                    intercept.path(),
                                    std::time::Duration::from_millis(100),
                                )
                                .await?;
                            }
                            if let Err(e) = reload_hook(&self.intercept) {
                                tracing::error!("Failed to recompile intercept hook: {:?}", e);
                            }
                        }
                    }
                    if let Some(authenticate) = self.authenticate.load().as_ref() {
                        if contains_path(&ev, authenticate.path()) {
                            if ev.kind.is_remove() {
                                // Add back the watch
                                watch_path_with_delay(
                                    &mut watcher,
                                    authenticate.path(),
                                    std::time::Duration::from_millis(100),
                                )
                                .await?;
                            }

                            if let Err(e) = reload_hook(&self.authenticate) {
                                tracing::error!("Failed to recompile authenticate hook: {:?}", e);
                            }
                        }
                    }
                }
                _ => continue,
            }
        }

        Ok(())
    }

    /// Reconciles hooks with the ones specified in the given configuration
    pub fn reconcile_hooks(&self, config: &Config) -> anyhow::Result<()> {
        let intercept_path = config.hooks.as_ref().and_then(|c| c.intercept.as_ref());
        let authenticate_path = config.hooks.as_ref().and_then(|c| c.authenticate.as_ref());

        reconcile_hook(&self.intercept, intercept_path)?;
        reconcile_hook(&self.authenticate, authenticate_path)?;

        Ok(())
    }

    /// Reconciles sources with the ones specified in the given configuration
    pub fn reconcile_sources(&self, config: &Config) -> anyhow::Result<()> {
        let mut sources = self.sources.lock().expect("poisoned lock");
        let mut seen = HashSet::new();

        for typ in config.sources.iter() {
            let id_incoming = typ.id();

            if !seen.insert(id_incoming) {
                return Err(anyhow::anyhow!(
                    "Found duplicate source ID in configuration: {}",
                    id_incoming
                ));
            }

            match sources.entry(id_incoming.clone()) {
                std::collections::btree_map::Entry::Occupied(_) => {
                    // Source already exists
                    continue;
                }
                std::collections::btree_map::Entry::Vacant(entry) => {
                    // Build and add source
                    let source = match typ {
                        SourceType::Kafka { topic, .. } => {
                            if let Some(kafka_config) = config.kafka.as_ref() {
                                <B as KafkaSourceBuilder>::build_source(
                                    typ.id().clone(),
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
            if !config.sources.iter().any(|typ| typ.id() == id) {
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
    fn test_parses_server_tls() {
        let config = "
        sources: []
        server:
            address: '127.0.0.1:8000'
            tls:
                cert: 'cert.pem'
                key: 'key.pem'
        ";

        let config = Config::from_str(config).unwrap();
        assert!(config.server.tls.is_some());
        let tls = config.server.tls.unwrap();
        assert_eq!(tls.cert, PathBuf::from("cert.pem"));
        assert_eq!(tls.key, PathBuf::from("key.pem"));
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
            matches!(config.sources[0].clone(), SourceType::Kafka { topic, .. } if topic == "test")
        );
        assert_eq!(config.sources[0].id(), "test");

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

        let config = "
        sources:
            - type: kafka
              topic: topic1
              id: test
        server:
            address: '127.0.0.1:8000'
        ";

        let config = Config::from_str(config).unwrap();
        assert_eq!(config.sources[0].id(), "test");
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

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    struct TestWasmHook;

    impl WasmHook for TestWasmHook {
        fn from_file<P: AsRef<Path>>(_path: P) -> Result<Self, anyhow::Error> {
            Ok(Self)
        }

        fn path(&self) -> &Path {
            "test".as_ref()
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
            _id: SourceId,
            topic: String,
            _bootstrap_servers: &[String],
            _group_id_prefix: &str,
        ) -> Result<Box<dyn Source + Send + Sync>, anyhow::Error> {
            Ok(Box::new(TestSource::new(&topic)))
        }
    }

    #[test]
    fn test_reconciliation_duplicate_sources_in_config() {
        let config_reconciler: ConfigReconciler<TestWasmHook, TestSourceBuilder, TestWasmHook> =
            ConfigReconciler::new(
                Arc::new(Mutex::new(BTreeMap::new())),
                Arc::new(ArcSwapOption::new(None)),
                Arc::new(ArcSwapOption::new(None)),
            );

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
                    id: None,
                },
            ],
            hooks: None,
            server: Server {
                address: "127.0.0.1:8000".into(),
                tls: None,
                healthcheck: false,
            },
            kafka: None,
            subscriber: Subscriber::default(),
        };

        assert!(config_reconciler.reconcile_sources(&config).is_err());
    }

    #[test]
    fn test_reconciliation_requires_kafka_config_if_kafka_source_present() {
        let config_reconciler: ConfigReconciler<TestWasmHook, TestSourceBuilder, TestWasmHook> =
            ConfigReconciler::new(
                Arc::new(Mutex::new(BTreeMap::new())),
                Arc::new(ArcSwapOption::new(None)),
                Arc::new(ArcSwapOption::new(None)),
            );

        let config = Config {
            sources: vec![SourceType::Kafka {
                topic: "test".into(),
                id: None,
            }],
            hooks: None,
            server: Server {
                address: "127.0.0.1:8000".into(),
                tls: None,
                healthcheck: false,
            },
            kafka: None,
            subscriber: Subscriber::default(),
        };

        assert!(config_reconciler.reconcile_sources(&config).is_err());

        let config = Config {
            sources: vec![SourceType::Kafka {
                topic: "test".into(),
                id: None,
            }],
            hooks: None,
            server: Server {
                address: "127.0.0.1:8000".into(),
                tls: None,
                healthcheck: false,
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
        let config_reconciler: ConfigReconciler<TestWasmHook, TestSourceBuilder, TestWasmHook> =
            ConfigReconciler::new(
                Arc::clone(&sources),
                Arc::new(ArcSwapOption::new(None)),
                Arc::new(ArcSwapOption::new(None)),
            );

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
                tls: None,
                healthcheck: false,
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
                "topic1".into(),
                &["localhost:9092".into()],
                "kiwi-",
            )
            .unwrap(),
        );

        let config_reconciler: ConfigReconciler<TestWasmHook, TestSourceBuilder, TestWasmHook> =
            ConfigReconciler::new(
                Arc::clone(&sources),
                Arc::new(ArcSwapOption::new(None)),
                Arc::new(ArcSwapOption::new(None)),
            );

        let config = Config {
            sources: vec![],
            hooks: None,
            server: Server {
                address: "127.0.0.1:8000".into(),
                tls: None,
                healthcheck: false,
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
                "topic1".into(),
                &["localhost:9092".into()],
                "kiwi-",
            )
            .unwrap(),
        );

        let config_reconciler: ConfigReconciler<TestWasmHook, TestSourceBuilder, TestWasmHook> =
            ConfigReconciler::new(
                Arc::clone(&sources),
                Arc::new(ArcSwapOption::new(None)),
                Arc::new(ArcSwapOption::new(None)),
            );

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
                tls: None,
                healthcheck: false,
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
    fn test_reconciliation_adds_hooks() {
        let config_reconciler: ConfigReconciler<TestWasmHook, TestSourceBuilder, TestWasmHook> =
            ConfigReconciler::new(
                Arc::new(Mutex::new(BTreeMap::new())),
                Arc::new(ArcSwapOption::new(None)),
                Arc::new(ArcSwapOption::new(None)),
            );

        let config = Config {
            sources: vec![],
            hooks: Some(Hooks {
                intercept: Some("test".into()),
                authenticate: Some("test".into()),
            }),
            server: Server {
                address: "127.0.0.1:8000".into(),
                tls: None,
                healthcheck: false,
            },
            kafka: None,
            subscriber: Subscriber::default(),
        };

        assert!(config_reconciler.reconcile_hooks(&config).is_ok());

        let intercept = config_reconciler.intercept.load();
        assert!(intercept.is_some());
        let authenticate = config_reconciler.authenticate.load();
        assert!(authenticate.is_some());
    }

    #[test]
    fn test_reconciliation_removes_intercept_hook() {
        let config_reconciler: ConfigReconciler<TestWasmHook, TestSourceBuilder, TestWasmHook> =
            ConfigReconciler::new(
                Arc::new(Mutex::new(BTreeMap::new())),
                Arc::new(ArcSwapOption::new(Some(Arc::new(TestWasmHook)))),
                Arc::new(ArcSwapOption::new(Some(Arc::new(TestWasmHook)))),
            );

        let config = Config {
            sources: vec![],
            hooks: None,
            server: Server {
                address: "127.0.0.1:8000".into(),
                tls: None,
                healthcheck: false,
            },
            kafka: None,
            subscriber: Subscriber::default(),
        };

        assert!(config_reconciler.reconcile_hooks(&config).is_ok());

        let intercept = config_reconciler.intercept.load();
        assert!(intercept.is_none());
        let authenticate = config_reconciler.authenticate.load();
        assert!(authenticate.is_none());
    }
}
