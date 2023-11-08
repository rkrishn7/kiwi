use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use clap::Parser;

use kiwi::config::Config;
use nanoid::nanoid;

use rdkafka::message::OwnedMessage;

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

    #[arg(short, long, default_value_t = tracing::Level::INFO, env)]
    pub log_level: tracing::Level,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    tracing_subscriber::fmt()
        .with_max_level(args.log_level)
        .init();

    let mut config = Config::parse(&args.config)?;

    let topic_senders = Arc::new(
        config
            .topics()
            .iter()
            .map(|topic| {
                let (tx, _) = tokio::sync::broadcast::channel::<OwnedMessage>(100);

                (topic.name.to_owned(), tx)
            })
            .collect::<HashMap<String, tokio::sync::broadcast::Sender<OwnedMessage>>>(),
    );

    let group_id = format!("{}{}", config.consumer_group_prefix(), nanoid!());

    // Add necessary overrides here
    config
        .consumer_config_mut()
        .insert("group.id".into(), group_id);

    let topics = config
        .topics()
        .iter()
        .map(|t| t.name.as_str())
        .collect::<Vec<&str>>();

    // TODO: Make worker count configurable
    kiwi::kafka::start_consumers(10, &topics, &config.consumer().config, &topic_senders)?;

    let listen_addr: SocketAddr = config.server_addr().parse()?;

    kiwi::ws::serve(&listen_addr, topic_senders).await?;

    Ok(())
}
