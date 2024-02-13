use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::Mutex;

use clap::Parser;

use kiwi::config::{Config, SourceType};
use kiwi::hook::wasm::{WasmAuthenticateHook, WasmInterceptHook};
use kiwi::source::counter::build_source as build_counter_source;
use kiwi::source::kafka::{build_source as build_kafka_source, start_partition_discovery};
use kiwi::source::Source;
use kiwi::source::SourceId;

/// kiwi is a bridge between your backend services and front-end applications.
/// It seamlessly and efficiently manages the flow of real-time Kafka events
/// through WebSockets, ensuring that your applications stay reactive and up-to-date
/// with the latest data.
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Path to the configuration file
    #[arg(short, long, env, default_value_t = String::from("/etc/kiwi/config/kiwi.yml"))]
    pub config: String,

    /// Log level
    #[arg(short, long, default_value_t = tracing::Level::INFO, env)]
    pub log_level: tracing::Level,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    tracing_subscriber::fmt()
        .with_max_level(args.log_level)
        .init();

    let config = Config::parse(&args.config)?;

    let sources: Arc<Mutex<BTreeMap<SourceId, Box<dyn Source + Send + Sync>>>> =
        Arc::new(Mutex::new(BTreeMap::new()));

    for source in config.sources.into_iter() {
        let (source_id, source) = match source {
            SourceType::Kafka { topic } => {
                if let Some(kafka_config) = config.kafka.as_ref() {
                    let source = build_kafka_source(
                        topic.clone(),
                        &kafka_config.bootstrap_servers,
                        &kafka_config.group_id_prefix,
                    )?;

                    (topic, source)
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
            } => {
                let source = build_counter_source(
                    id.clone(),
                    min,
                    max,
                    std::time::Duration::from_millis(interval_ms),
                    lazy,
                );

                (id, source)
            }
        };

        match sources
            .lock()
            .expect("poisoned lock")
            .entry(source_id.clone())
        {
            std::collections::btree_map::Entry::Occupied(_) => {
                panic!("Found duplicate source ID in configuration: {}", source_id);
            }
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(source);
            }
        }
    }

    if let Some(kafka_config) = config.kafka.as_ref() {
        if kafka_config.partition_discovery_enabled {
            start_partition_discovery(
                &kafka_config.bootstrap_servers,
                Arc::clone(&sources),
                std::time::Duration::from_millis(
                    kafka_config.partition_discovery_interval_ms.into(),
                ),
            )?;
        }
    }

    let adapter_path = config
        .hooks
        .as_ref()
        .and_then(|c| c.__adapter_path.as_ref());

    let intercept_hook = config
        .hooks
        .as_ref()
        .and_then(|hooks| hooks.intercept.clone())
        .map(|path| {
            WasmInterceptHook::from_file(path, adapter_path.cloned())
                .expect("failed to load intercept wasm hook")
        });

    let authenticate_hook = config
        .hooks
        .as_ref()
        .and_then(|hooks| hooks.authenticate.clone())
        .map(|path| {
            WasmAuthenticateHook::from_file(path, adapter_path.cloned())
                .expect("failed to load authenticate wasm hook")
        });

    let listen_addr: SocketAddr = config.server.address.parse()?;

    kiwi::ws::serve(
        &listen_addr,
        sources,
        intercept_hook,
        authenticate_hook,
        config.subscriber,
    )
    .await?;

    Ok(())
}
