use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::Mutex;

use arc_swap::ArcSwapOption;
use clap::Parser;

use kiwi::config::Config;
use kiwi::config::ConfigReconciler;
use kiwi::source::kafka::start_partition_discovery;
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

    let config_path = args.config.clone();

    let config = Config::parse(&config_path)?;

    let sources: Arc<Mutex<BTreeMap<SourceId, Box<dyn Source + Send + Sync>>>> =
        Arc::new(Mutex::new(BTreeMap::new()));

    let intercept = Arc::new(ArcSwapOption::new(None));
    let authenticate = Arc::new(ArcSwapOption::new(None));

    let config_reconciler: ConfigReconciler = ConfigReconciler::new(
        Arc::clone(&sources),
        Arc::clone(&intercept),
        Arc::clone(&authenticate),
    );

    config_reconciler.reconcile_sources(&config)?;
    config_reconciler.reconcile_hooks(&config)?;

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

    tokio::spawn(async move {
        if let Err(e) = config_reconciler.watch(config_path.into()).await {
            tracing::error!(
                "Configuration watcher exited unexpectedly with the error: {}",
                e
            );
        }
    });

    let listen_addr: SocketAddr = config.server.address.parse()?;

    #[cfg(windows)]
    let mut term = tokio::signal::windows::ctrl_close().unwrap();
    #[cfg(unix)]
    let mut term =
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).unwrap();

    tokio::select! {
        _ = term.recv() => {
            tracing::info!("Received SIGTERM, shutting down");
        }
        _ = tokio::signal::ctrl_c() => {
            tracing::info!("Received SIGINT, shutting down");
        }
        _ = kiwi::ws::serve(
            &listen_addr,
            sources,
            intercept,
            authenticate,
            config.subscriber,
            config.server.tls,
        ) => {}
    }

    Ok(())
}
