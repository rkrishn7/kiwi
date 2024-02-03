use futures::{SinkExt, StreamExt};
use rdkafka::{
    admin::{AdminClient, AdminOptions, NewPartitions},
    client::DefaultClientContext,
    producer::{FutureProducer, FutureRecord},
};
use std::{io::Write, process, time::Duration};
use tokio_tungstenite::{connect_async, tungstenite};

use kiwi::protocol::{Command, CommandResponse, Message, Notice, SubscriptionMode};

use tempfile::NamedTempFile;

// Helper function to start the kiwi process.
fn start_kiwi(config_file: &str) -> anyhow::Result<std::process::Child> {
    // Expects `kiwi` to be in the PATH
    let mut cmd = process::Command::new("kiwi");

    cmd.args(&["--config", config_file, "--log-level", "debug"]);
    let child = cmd.spawn().expect("kiwi failed to start");

    Ok(child)
}

fn create_temp_config(contents: &str) -> anyhow::Result<NamedTempFile> {
    let mut config_file = NamedTempFile::new()?;
    config_file.as_file_mut().write_all(contents.as_bytes())?;

    Ok(config_file)
}

fn create_admin_client() -> rdkafka::admin::AdminClient<DefaultClientContext> {
    let mut admin_client_config = rdkafka::ClientConfig::new();

    admin_client_config
        .set("bootstrap.servers", "kafka:19092")
        .set("message.timeout.ms", "5000");

    let admin_client: rdkafka::admin::AdminClient<DefaultClientContext> =
        admin_client_config.create().unwrap();

    admin_client
}

async fn create_topic(topic_name: &str, admin_client: &AdminClient<DefaultClientContext>) {
    let topic =
        rdkafka::admin::NewTopic::new(topic_name, 1, rdkafka::admin::TopicReplication::Fixed(1));

    admin_client
        .create_topics(&[topic], &AdminOptions::new())
        .await
        .expect(format!("Failed to create topic {}", topic_name).as_str());
}

#[tokio::test]
async fn test_kafka_source() -> anyhow::Result<()> {
    let config = create_temp_config(
        r#"
sources:
    - type: kafka
      topic: topic1

kafka:
    bootstrap_servers:
        - 'kafka:19092'
server:
    address: '127.0.0.1:8000'
    "#,
    )
    .unwrap();

    let admin_client = create_admin_client();
    create_topic("topic1", &admin_client).await;

    let mut proc = start_kiwi(config.path().to_str().unwrap())?;

    tokio::time::sleep(Duration::from_secs(2)).await;

    let (ws_stream, _) = connect_async("ws://127.0.0.1:8000")
        .await
        .expect("Failed to connect");

    let (mut write, mut read) = ws_stream.split();

    let cmd = Command::Subscribe {
        source_id: "topic1".into(),
        mode: SubscriptionMode::Push,
    };

    write
        .send(tungstenite::protocol::Message::Text(
            serde_json::to_string(&cmd).unwrap(),
        ))
        .await?;

    let resp = read.next().await.expect("Expected response")?;

    let resp: Message = serde_json::from_str(&resp.to_text().unwrap())?;

    match resp {
        Message::CommandResponse(CommandResponse::SubscribeOk { source_id }) => {
            assert_eq!(source_id, "topic1".to_string());
        }
        _ => panic!("Expected subscribe ok"),
    }

    let producer = tokio::spawn(async {
        let producer: FutureProducer = rdkafka::config::ClientConfig::new()
            .set("bootstrap.servers", "kafka:19092")
            .set("message.timeout.ms", "5000")
            .create()
            .expect("Producer creation error");

        for i in 0..1000 {
            let payload = format!("Message {}", i);
            let key = format!("Key {}", i);
            let record = FutureRecord::to("topic1").payload(&payload).key(&key);

            producer
                .send(record, Duration::from_secs(0))
                .await
                .expect("Failed to enqueue");
        }
    });

    let reader = tokio::spawn(async move {
        let mut count = 0;
        while count < 1000 {
            let msg = read.next().await.unwrap().unwrap();
            let msg: Message = serde_json::from_str(&msg.to_text().unwrap()).unwrap();

            match msg {
                Message::Result(msg) => {
                    assert_eq!(msg.source_id.as_ref(), "topic1".to_string());
                    assert_eq!(
                        std::str::from_utf8(&msg.payload.unwrap()).unwrap(),
                        format!("Message {}", count)
                    );
                    count += 1;
                }
                m => panic!("Expected message. Received {:?}", m),
            }
        }
    });

    assert!(matches!(futures::join!(producer, reader), (Ok(_), Ok(_))));

    let _ = proc.kill();

    Ok(())
}

#[tokio::test]
async fn test_closes_subscription_on_changed_metadata() -> anyhow::Result<()> {
    let config = create_temp_config(
        r#"
sources:
    - type: kafka
      topic: topic1

kafka:
    partition_discovery_enabled: true
    partition_discovery_interval_ms: 5000
    bootstrap_servers:
        - 'kafka:19092'
server:
    address: '127.0.0.1:8000'
    "#,
    )
    .unwrap();

    let admin_client = create_admin_client();
    create_topic("topic1", &admin_client).await;

    let mut proc = start_kiwi(config.path().to_str().unwrap())?;

    tokio::time::sleep(Duration::from_secs(2)).await;

    let (ws_stream, _) = connect_async("ws://127.0.0.1:8000")
        .await
        .expect("Failed to connect");

    let (mut write, mut read) = ws_stream.split();

    let cmd = Command::Subscribe {
        source_id: "topic1".into(),
        mode: SubscriptionMode::Push,
    };

    write
        .send(tungstenite::protocol::Message::Text(
            serde_json::to_string(&cmd).unwrap(),
        ))
        .await?;

    let resp = read.next().await.expect("Expected response")?;

    let resp: Message = serde_json::from_str(&resp.to_text().unwrap())?;

    match resp {
        Message::CommandResponse(CommandResponse::SubscribeOk { source_id }) => {
            assert_eq!(source_id, "topic1".to_string());
        }
        _ => panic!("Expected subscribe ok"),
    }

    let mut partitions = Vec::new();
    partitions.push(&NewPartitions {
        topic_name: "topic1",
        new_partition_count: 2,
        assignment: None,
    });

    admin_client
        .create_partitions(partitions, &AdminOptions::new())
        .await
        .unwrap();

    let resp = tokio::select! {
        resp = read.next() => resp.expect("Expected response")?,
        _ = tokio::time::sleep(Duration::from_secs(7)) => panic!("Expected timely response"),
    };

    let resp: Message = serde_json::from_str(&resp.to_text().unwrap())?;

    match resp {
        Message::Notice(Notice::SubscriptionClosed { source, message: _ }) => {
            assert_eq!(source, "topic1".to_string());
        }
        _ => panic!("Expected subscribe closed"),
    }

    let _ = proc.kill();

    Ok(())
}
