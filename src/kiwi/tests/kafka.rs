pub mod common;

use std::io::{Seek, SeekFrom, Write};
use std::time::Duration;

use common::kafka::AdminClient;
use common::kiwi::{ConfigFile, Process};
use common::ws::Client as WsClient;
use kiwi::protocol::{Command, CommandResponse, Message, Notice, SubscriptionMode};
use once_cell::sync::Lazy;

use crate::common::healthcheck::Healthcheck;
use crate::common::kafka::Producer;

static BOOTSTRAP_SERVER: Lazy<String> = Lazy::new(|| {
    std::env::var("BOOTSTRAP_SERVERS")
        .expect("BOOTSTRAP_SERVERS env var is required")
        .split(',')
        .next()
        .unwrap()
        .to_string()
});

/// Tests that the subscriber (client) receives messages from the specified
/// Kafka topic
#[tokio::test]
async fn test_receives_messages_kafka_source() -> anyhow::Result<()> {
    let bootstrap_server = BOOTSTRAP_SERVER.as_str();
    let mut client = AdminClient::new(bootstrap_server)?;
    let topic = client.create_random_topic(1).await?;
    let config = ConfigFile::from_str(
        format!(
            r#"
        sources:
            - type: kafka
              topic: {topic}

        kafka:
            bootstrap_servers:
                - '{bootstrap_server}'
        server:
            address: '127.0.0.1:8000'
        "#
        )
        .as_str(),
    )?;
    let _kiwi = Process::new_with_args(&["--config", config.path_str()])?;

    Healthcheck {
        interval: Duration::from_millis(200),
        attempts: 10,
        url: "http://127.0.0.1:8000/health",
    }
    .run()
    .await?;

    let (mut ws_client, _) = WsClient::connect("ws://127.0.0.1:8000").await?;

    ws_client
        .send_json(&Command::Subscribe {
            source_id: topic.clone(),
            mode: SubscriptionMode::Push,
        })
        .await?;

    let resp: Message = ws_client.recv_json().await?;

    assert!(
        matches!(resp, kiwi::protocol::Message::CommandResponse(CommandResponse::SubscribeOk { source_id }) if source_id == topic)
    );

    let producer = tokio::spawn({
        let topic = topic.clone();
        async move {
            let producer = Producer::new(bootstrap_server)?;

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
            let msg = ws_client.recv_json::<Message>().await?;

            match msg {
                Message::Result(kiwi::protocol::SourceResult::Kafka {
                    source_id, payload, ..
                }) => {
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

    assert!(matches!(
        futures::join!(producer, consumer),
        (Ok(Ok(_)), Ok(Ok(_)))
    ));

    Ok(())
}

/// Tests that adding a new partition to a Kafka topic closes any outstanding
/// subscriptions to the respective source. Partition discovery runs on a fixed
/// interval, and a new partition can indicate that not all data from the time
/// the subscription was opened has been observed. In this case, we close the
/// subscription so the client can perform any initialization logic before potentially
/// re-subscribing.
#[tokio::test]
async fn test_closes_subscription_on_partition_added() -> anyhow::Result<()> {
    let bootstrap_server = BOOTSTRAP_SERVER.as_str();
    let mut client = AdminClient::new(bootstrap_server)?;
    let topic = client.create_random_topic(1).await?;
    let config = ConfigFile::from_str(
        format!(
            r#"
        sources:
            - type: kafka
              topic: {topic}

        kafka:
            bootstrap_servers:
                - '{bootstrap_server}'
            partition_discovery_interval_ms: 3000
        server:
            address: '127.0.0.1:8000'
        "#
        )
        .as_str(),
    )?;
    let _kiwi = Process::new_with_args(&["--config", config.path_str()])?;

    Healthcheck {
        interval: Duration::from_millis(200),
        attempts: 10,
        url: "http://127.0.0.1:8000/health",
    }
    .run()
    .await?;

    let (mut ws_client, _) = WsClient::connect("ws://127.0.0.1:8000").await?;

    ws_client
        .send_json(&Command::Subscribe {
            source_id: topic.clone(),
            mode: SubscriptionMode::Push,
        })
        .await?;

    let resp: Message = ws_client.recv_json().await?;

    assert!(
        matches!(resp, kiwi::protocol::Message::CommandResponse(CommandResponse::SubscribeOk { source_id }) if source_id == topic)
    );

    client.update_partitions(&topic, 2).await?;

    let resp: Message = tokio::select! {
        resp = ws_client.recv_json() => resp?,
        _ = tokio::time::sleep(Duration::from_secs(7)) => panic!("Expected timely response"),
    };

    match resp {
        Message::Notice(Notice::SubscriptionClosed {
            source_id,
            message: _,
        }) => {
            assert_eq!(source_id, topic);
        }
        _ => panic!("Expected subscription closed notice"),
    }

    Ok(())
}

/// Tests that specifying a name for a Kafka source is allowed, and that
/// all subscription requests and messages use the specified source name
/// rather than the topic name
#[tokio::test]
async fn test_named_kafka_source() -> anyhow::Result<()> {
    let bootstrap_server = BOOTSTRAP_SERVER.as_str();
    let mut client = AdminClient::new(bootstrap_server)?;
    let topic = client.create_random_topic(1).await?;
    let config = ConfigFile::from_str(
        format!(
            r#"
        sources:
            - type: kafka
              id: my-kafka-source
              topic: {topic}

        kafka:
            bootstrap_servers:
                - '{bootstrap_server}'
        server:
            address: '127.0.0.1:8000'
        "#
        )
        .as_str(),
    )?;
    let _kiwi = Process::new_with_args(&["--config", config.path_str()])?;

    Healthcheck {
        interval: Duration::from_millis(200),
        attempts: 10,
        url: "http://127.0.0.1:8000/health",
    }
    .run()
    .await?;

    let (mut ws_client, _) = WsClient::connect("ws://127.0.0.1:8000").await?;

    ws_client
        .send_json(&Command::Subscribe {
            source_id: topic.clone(),
            mode: SubscriptionMode::Push,
        })
        .await?;

    assert!(matches!(
        ws_client.recv_json().await?,
        Message::CommandResponse(CommandResponse::SubscribeError { .. })
    ));

    ws_client
        .send_json(&Command::Subscribe {
            source_id: "my-kafka-source".to_string(),
            mode: SubscriptionMode::Push,
        })
        .await?;

    assert!(matches!(
        ws_client.recv_json().await?,
        Message::CommandResponse(CommandResponse::SubscribeOk { source_id }) if source_id == "my-kafka-source"
    ));

    let producer = Producer::new(bootstrap_server)?;

    producer.send(&topic, "key", "value").await?;

    assert!(matches!(
        ws_client.recv_json().await?,
        Message::Result(kiwi::protocol::SourceResult::Kafka { source_id, .. }) if source_id == "my-kafka-source"
    ));

    Ok(())
}

/// Tests removing a Kafka source from the configuration triggers a reconciliation
/// of the sources and closes any outstanding subscriptions to the respective source
#[tokio::test]
async fn test_dynamic_config_source_removal() -> anyhow::Result<()> {
    let bootstrap_server = BOOTSTRAP_SERVER.as_str();
    let mut client = AdminClient::new(bootstrap_server)?;
    let topic = client.create_random_topic(1).await?;
    let mut config = ConfigFile::from_str(
        format!(
            r#"
        sources:
            - type: kafka
              topic: {topic}
        kafka:
            bootstrap_servers:
                - '{bootstrap_server}'
        server:
            address: '127.0.0.1:8000'
        "#
        )
        .as_str(),
    )?;
    let _kiwi = Process::new_with_args(&["--config", config.path_str()])?;

    Healthcheck {
        interval: Duration::from_millis(200),
        attempts: 10,
        url: "http://127.0.0.1:8000/health",
    }
    .run()
    .await?;

    let (mut ws_client, _) = WsClient::connect("ws://127.0.0.1:8000").await?;

    ws_client
        .send_json(&Command::Subscribe {
            source_id: topic.clone(),
            mode: SubscriptionMode::Push,
        })
        .await?;

    let resp: Message = ws_client.recv_json().await?;

    assert!(
        matches!(resp, kiwi::protocol::Message::CommandResponse(CommandResponse::SubscribeOk { source_id }) if source_id == topic)
    );

    config.as_file_mut().set_len(0)?;
    config.as_file_mut().seek(SeekFrom::Start(0))?;

    config.as_file_mut().write_all(
        format!(
            r#"
        sources: []
        kafka:
            bootstrap_servers:
                - 'kafka:19092'
        server:
            address: '127.0.0.1:8000'
        "#
        )
        .as_bytes(),
    )?;

    config.as_file_mut().flush()?;

    // Close the file, without removing it from the filesystem
    let tmp_path = config.into_temp_path();
    tmp_path.keep()?;

    assert!(
        matches!(ws_client.recv_json().await?, kiwi::protocol::Message::Notice(Notice::SubscriptionClosed { source_id, .. }) if source_id == topic)
    );

    ws_client
        .send_json(&Command::Subscribe {
            source_id: topic.clone(),
            mode: SubscriptionMode::Push,
        })
        .await?;

    let resp: Message = ws_client.recv_json().await?;

    assert!(matches!(
        resp,
        kiwi::protocol::Message::CommandResponse(CommandResponse::SubscribeError { .. })
    ));

    Ok(())
}

/// Regression test of intercept plugins for Kafka sources
#[tokio::test]
async fn test_intercept_hook() -> anyhow::Result<()> {
    const INTERCEPT_PATH: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/wasm/kafka-even-numbers-intercept.wasm"
    );

    let bootstrap_server = BOOTSTRAP_SERVER.as_str();
    let mut client = AdminClient::new(bootstrap_server)?;
    let topic = client.create_random_topic(1).await?;
    let config = ConfigFile::from_str(
        format!(
            r#"
        hooks:
            intercept: {INTERCEPT_PATH}
        sources:
            - type: kafka
              topic: {topic}

        kafka:
            bootstrap_servers:
                - '{bootstrap_server}'
        server:
            address: '127.0.0.1:8000'
        "#
        )
        .as_str(),
    )?;
    let _kiwi = Process::new_with_args(&["--config", config.path_str()])?;

    Healthcheck {
        interval: Duration::from_millis(200),
        attempts: 10,
        url: "http://127.0.0.1:8000/health",
    }
    .run()
    .await?;

    let (mut ws_client, _) = WsClient::connect("ws://127.0.0.1:8000").await?;

    ws_client
        .send_json(&Command::Subscribe {
            source_id: topic.clone(),
            mode: SubscriptionMode::Push,
        })
        .await?;

    let resp: Message = ws_client.recv_json().await?;

    assert!(
        matches!(resp, kiwi::protocol::Message::CommandResponse(CommandResponse::SubscribeOk { source_id }) if source_id == topic)
    );

    let producer = tokio::spawn({
        let topic = topic.clone();
        async move {
            let producer = Producer::new(bootstrap_server)?;

            for i in 0..1000 {
                let payload = format!("{i}");
                producer.send(&topic, &payload, &payload).await?;
            }

            Ok::<_, anyhow::Error>(())
        }
    });

    let consumer = tokio::spawn(async move {
        loop {
            let msg = ws_client.recv_json::<Message>().await?;

            match msg {
                Message::Result(kiwi::protocol::SourceResult::Kafka {
                    source_id, payload, ..
                }) => {
                    assert_eq!(source_id.as_ref(), topic);

                    let payload: i32 = std::str::from_utf8(&payload.unwrap())?.parse()?;

                    assert_eq!(payload % 2, 0);

                    if payload == 998 {
                        break;
                    }
                }
                _ => panic!("Expected Kafka message. Received {:?}", msg),
            }
        }

        Ok::<_, anyhow::Error>(())
    });

    assert!(matches!(
        futures::join!(producer, consumer),
        (Ok(Ok(_)), Ok(Ok(_)))
    ));

    Ok(())
}
