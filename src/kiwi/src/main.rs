use std::net::SocketAddr;
use std::sync::Arc;

use clap::Parser;

use kiwi::config::Config;
use kiwi::config::Kafka as KafkaConfig;
use kiwi::hook::authenticate::wasm::WasmAuthenticateHook;
use kiwi::hook::intercept::wasm::WasmInterceptHook;
use kiwi::source::kafka::build_sources as build_kafka_sources;

/// kiwi is a bridge between your backend services and front-end applications.
/// It seamlessly and efficiently manages the flow of real-time Kafka events
/// through WebSockets, ensuring that your applications stay reactive and up-to-date
/// with the latest data.
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Path to the configuration file
    #[arg(short, long, env, default_value_t = String::from("kiwi.yml"))]
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

    let KafkaConfig {
        topics,
        group_prefix,
        bootstrap_servers,
    } = &config.sources.kafka;

    // Build Kafka sources
    // TODO: Once we introduce more sources, figure out how to make this
    // play nice with dynamic dispatch
    let kafka_sources = build_kafka_sources(
        topics.iter().map(|topic| topic.name.clone()),
        group_prefix.clone(),
        bootstrap_servers.clone(),
    );

    let intercept_hook = config
        .hooks
        .as_ref()
        .and_then(|hooks| hooks.intercept.clone())
        .map(|path| {
            WasmInterceptHook::from_file(path).expect("failed to load intercept wasm hook")
        });

    let authenticate_hook = config
        .hooks
        .and_then(|hooks| hooks.authenticate)
        .map(|path| {
            WasmAuthenticateHook::from_file(path).expect("failed to load authenticate wasm hook")
        });

    let listen_addr: SocketAddr = config.server.address.parse()?;

    kiwi::ws::serve(
        &listen_addr,
        Arc::new(kafka_sources),
        intercept_hook,
        authenticate_hook,
    )
    .await?;

    Ok(())
}
