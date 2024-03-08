mod common;

use std::time::Duration;

use common::kafka::AdminClient;
use common::kiwi::{ConfigFile, Process};
use common::ws::Client as WsClient;
use kiwi::protocol::{Command, CommandResponse, SubscriptionMode};
use once_cell::sync::Lazy;

use crate::common::kafka::Producer;

static BOOTSTRAP_SERVERS: Lazy<String> = Lazy::new(|| {
    std::env::var("BOOTSTRAP_SERVERS").expect("BOOTSTRAP_SERVERS env var is required")
});

/// Tests that the subscriber (client) receives messages from the specified
/// Kafka topic
#[tokio::test]
async fn test_receives_messages_kafka_source() -> anyhow::Result<()> {
    let mut client = AdminClient::new(BOOTSTRAP_SERVERS.as_str())?;
    let topic = client.create_random_topic(1).await?;
    let config = ConfigFile::from_str(
        format!(
            r#"
        sources:
            - type: kafka
            topic: {topic}

        kafka:
            bootstrap_servers:
                - 'kafka:19092'
        server:
            address: '127.0.0.1:8000'
        "#
        )
        .as_str(),
    )?;
    let _kiwi = Process::new_with_args(&["--config", config.path_str()])?;

    // TODO: Replace with a proper health check
    tokio::time::sleep(Duration::from_secs(2)).await;

    let (mut ws_client, _) = WsClient::connect("ws://127.0.0.1:8000").await?;

    ws_client
        .send_json(&Command::Subscribe {
            source_id: topic.clone(),
            mode: SubscriptionMode::Push,
        })
        .await?;

    let resp: CommandResponse = ws_client.recv_json().await?;

    assert!(matches!(resp, CommandResponse::SubscribeOk { source_id } if source_id == topic));

    let producer = tokio::spawn({
        let topic = topic.clone();
        async move {
            let producer = Producer::new(BOOTSTRAP_SERVERS.as_str())?;

            for i in 0..1000 {
                let payload = format!("Message {}", i);
                let key = format!("Key {}", i);
                producer.send(&topic, &key, &payload).await?;
            }

            Ok::<_, anyhow::Error>(())
        }
    });

    let consumer = tokio::spawn(async move {
        for count in 0..1000 {
            let msg = ws_client
                .recv_json::<kiwi::protocol::SourceResult>()
                .await?;

            match msg {
                kiwi::protocol::SourceResult::Kafka {
                    source_id, payload, ..
                } => {
                    assert_eq!(source_id.as_ref(), topic);
                    assert_eq!(
                        std::str::from_utf8(&payload.unwrap()).unwrap(),
                        format!("Message {}", count)
                    );
                }
                _ => panic!("Expected Kafka message. Received {:?}", msg),
            }
        }

        Ok::<_, anyhow::Error>(())
    });

    assert!(matches!(futures::join!(producer, consumer), (Ok(_), Ok(_))));

    Ok(())
}
