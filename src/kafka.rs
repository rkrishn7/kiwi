use std::collections::HashMap;
use std::sync::Arc;

use futures::stream::StreamExt;
use rdkafka::consumer::{Consumer, StreamConsumer};
use rdkafka::message::{Message, OwnedMessage};
use rdkafka::ClientConfig;
use tokio::sync::broadcast::Sender;

pub fn start_consumers(
    num_workers: usize,
    topics: &Vec<&str>,
    properties: &HashMap<String, String>,
    topic_senders: &Arc<HashMap<String, Sender<OwnedMessage>>>,
) -> anyhow::Result<()> {
    for _ in 0..num_workers {
        let topic_senders: Arc<HashMap<String, Sender<OwnedMessage>>> = Arc::clone(topic_senders);
        let mut client_config = ClientConfig::new();
        client_config.extend(properties.clone());

        let consumer: StreamConsumer = client_config.create()?;

        consumer.subscribe(topics)?;

        tokio::spawn(async move {
            let mut stream = consumer.stream();

            while let Some(e) = stream.next().await {
                match e {
                    Err(err) => {
                        tracing::error!("Encountered Kafka error while yielding messages: {}", err);
                    }
                    Ok(borrowed_message) => {
                        let owned = borrowed_message.detach();

                        if let Some(tx) = topic_senders.get(owned.topic()) {
                            // Erroring does not mean future calls will fail, since new subscribers
                            // may be created. If there are no subscribers, we simply discard the message
                            // and move on
                            let _ = tx.send(owned);
                        }
                    }
                };
            }
        });
    }

    Ok(())
}
