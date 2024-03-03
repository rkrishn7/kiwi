use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use std::sync::Mutex;

use clap::Parser;

use kiwi::config::Config;
use kiwi::config::ConfigReconciler;
use kiwi::hook::wasm::{WasmAuthenticateHook, WasmInterceptHook};
use kiwi::source::kafka::start_partition_discovery;
use kiwi::source::Source;
use kiwi::source::SourceId;
use notify::RecommendedWatcher;
use notify::Watcher;

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

    let config_path = args.config.clone();

    let config = Config::parse(&config_path)?;

    let sources: Arc<Mutex<BTreeMap<SourceId, Box<dyn Source + Send + Sync>>>> =
        Arc::new(Mutex::new(BTreeMap::new()));

    let config_reconciler: ConfigReconciler = ConfigReconciler::new(Arc::clone(&sources));

    config_reconciler.reconcile_sources(&config)?;

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

    let _watcher = init_config_watcher(config_reconciler, args.config)?;

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

fn init_config_watcher<P: AsRef<Path> + Clone + Send + 'static>(
    config_reconciler: ConfigReconciler,
    path: P,
) -> anyhow::Result<RecommendedWatcher> {
    let (tx, mut rx) = tokio::sync::watch::channel(());

    let mut watcher = RecommendedWatcher::new(
        move |res: notify::Result<notify::Event>| {
            if let Ok(event) = res {
                match event.kind {
                    notify::EventKind::Modify(_) => {
                        tx.send(()).expect("config update channel closed");
                    }
                    _ => (),
                }
            }
        },
        notify::Config::default(),
    )?;

    tokio::spawn({
        let path = path.clone();
        async move {
            while rx.changed().await.is_ok() {
                if let Err(e) = Config::parse(&path)
                    .and_then(|config| config_reconciler.reconcile_sources(&config))
                {
                    tracing::error!("Failed to reload config: {}", e);
                    continue;
                }

                tracing::info!("Configuration changes applied");
            }

            tracing::error!("config update sender dropped");
        }
    });

    watcher.watch(path.as_ref(), notify::RecursiveMode::NonRecursive)?;

    Ok(watcher)
}
