use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use clap::Parser;

use futures::stream::StreamExt;
use kiwi::config::Config;
use kiwi::topic::TopicBroadcastChannel;
use nanoid::nanoid;

use rdkafka::config::ClientConfig;
use rdkafka::consumer::stream_consumer::StreamConsumer;
use rdkafka::consumer::Consumer;
use rdkafka::message::{Message, OwnedMessage};

/// TODO
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

    let group_id = format!("{}{}", config.consumer_group_prefix(), nanoid!());

    // Add necessary overrides here
    config
        .consumer_config_mut()
        .insert("group.id".into(), group_id);

    let topic_broadcast_map = Arc::new(
        config
            .topics()
            .iter()
            .map(|topic| {
                (
                    topic.name.to_owned(),
                    tokio::sync::broadcast::channel::<OwnedMessage>(100).into(),
                )
            })
            .collect::<HashMap<String, TopicBroadcastChannel<OwnedMessage>>>(),
    );

    let listen_addr: SocketAddr = config.server_addr().parse()?;

    // Worker count
    for _ in 0..10 {
        let topic_broadcast_map: Arc<HashMap<String, TopicBroadcastChannel<OwnedMessage>>> =
            Arc::clone(&topic_broadcast_map);
        let mut client_config = ClientConfig::new();
        client_config.extend(config.consumer().config.clone().into_iter());

        let consumer: StreamConsumer = client_config.create()?;

        consumer
            .subscribe(
                &config
                    .topics()
                    .iter()
                    .map(|t| t.name.as_str())
                    .collect::<Vec<&str>>(),
            )
            .expect("Can't subscribe to topics");

        tokio::spawn(async move {
            while let Some(e) = consumer.stream().next().await {
                match e {
                    Err(err) => {
                        println!("Kafka error: {}", err);
                    }
                    Ok(borrowed_message) => {
                        let owned = borrowed_message.detach();

                        if let Some(bm) = topic_broadcast_map.get(owned.topic()) {
                            bm.tx.send(owned).unwrap();
                        }
                    }
                };
            }
        });
    }

    kiwi::server::serve(&listen_addr, topic_broadcast_map).await?;

    Ok(())
}
