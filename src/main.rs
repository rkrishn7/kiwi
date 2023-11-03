use std::borrow::Borrow;
use std::collections::HashMap;
use std::sync::Arc;
use std::{env, io::Error};

use futures::future;
use futures::stream::{StreamExt, TryStreamExt};
use kiwi::config::Config;
use tokio::net::{TcpListener, TcpStream};

use rdkafka::config::ClientConfig;
use rdkafka::consumer::stream_consumer::StreamConsumer;
use rdkafka::consumer::Consumer;
use rdkafka::message::{BorrowedMessage, Message, OwnedMessage};
use rdkafka::producer::{FutureProducer, FutureRecord};

use kiwi::server;

#[tokio::main]
async fn main() -> Result<(), Error> {
    let addr = env::args()
        .nth(1)
        .unwrap_or_else(|| "127.0.0.1:8080".to_string());

    let config = Arc::new(Config::parse());

    let mut map = HashMap::new();

    for topic in config.topics() {
        let (tx, rx) = tokio::sync::broadcast::channel::<OwnedMessage>(100);
        map.insert(topic.to_owned(), (tx, rx));
    }

    let map = Arc::new(map);

    // Worker count
    for i in 0..10 {
        let config = Arc::clone(&config);
        let map = map.clone();

        tokio::spawn(async move {
            // Create the `StreamConsumer`, to receive the messages from the topic in form of a `Stream`.
            let consumer: StreamConsumer = ClientConfig::new()
                .set("group.id", "test")
                .set("bootstrap.servers", &config.brokers_list)
                .set("enable.partition.eof", "false")
                .set("session.timeout.ms", "6000")
                .set("enable.auto.commit", "false")
                .create()
                .expect("Consumer creation failed");

            consumer
                .subscribe(&config.topics()[..])
                .expect("Can't subscribe to specified topic");

            while let Some(e) = consumer.stream().next().await {
                match e {
                    Err(err) => {
                        println!("Kafka error: {}", err);
                    }
                    Ok(borrowed_message) => {
                        let owned = borrowed_message.detach();

                        if let Some((tx, _)) = map.get(owned.topic()) {
                            tx.send(owned).unwrap();
                        }
                    }
                };
            }
        });
    }

    // Create the event loop and TCP listener we'll accept connections on.
    let try_socket = TcpListener::bind(&addr).await;
    let listener = try_socket.expect("Failed to bind");
    tracing::info!("Listening on: {}", addr);

    while let Ok((stream, _)) = listener.accept().await {
        tokio::spawn(accept_connection(stream));
    }

    Ok(())
}

async fn accept_connection(stream: TcpStream) {
    let addr = stream
        .peer_addr()
        .expect("connected streams should have a peer address");
    tracing::info!("Peer address: {}", addr);

    let ws_stream = tokio_tungstenite::accept_async(stream)
        .await
        .expect("Error during the websocket handshake occurred");

    tracing::info!("New WebSocket connection: {}", addr);

    let (write, read) = ws_stream.split();
    // We should not forward messages other than text or binary.
    read.try_filter(|msg| future::ready(msg.is_text() || msg.is_binary()))
        .forward(write)
        .await
        .expect("Failed to forward messages")
}
