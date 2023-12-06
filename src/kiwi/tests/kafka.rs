use base64::Engine;
use futures::{SinkExt, StreamExt};
use rdkafka::producer::{FutureProducer, FutureRecord};
use std::{io::Write, process, time::Duration};
use tokio_tungstenite::{connect_async, tungstenite};

use kiwi::{
    protocol::{Command, CommandResponse, Message},
    ws::MessageData,
};

use tempfile::NamedTempFile;

// Helper function to start the kiwi process.
fn start_kiwi(config: &str) -> anyhow::Result<std::process::Child> {
    // Expects `kiwi` to be in the PATH
    let mut cmd = process::Command::new("kiwi");

    let mut config_file = NamedTempFile::new()?;
    config_file
        .as_file_mut()
        .write_all(config.as_bytes())
        .unwrap();

    cmd.args(&[
        "--config",
        config_file.path().to_str().expect("path should be valid"),
        "--log-level",
        "debug",
    ]);
    let child = cmd.spawn().unwrap();

    Ok(child)
}

#[ignore = "todo"]
#[tokio::test]
async fn test_kafka_source() -> anyhow::Result<()> {
    let config = r#"
    sources:
        kafka:
            group_prefix: ''
            bootstrap_servers:
                - 'localhost:9092'
            topics:
                - name: topic1
  
    server:
        address: '127.0.0.1:8000'
    "#;

    let mut proc = start_kiwi(config)?;

    let (ws_stream, _) = connect_async("http://127.0.0.1:8000")
        .await
        .expect("Failed to connect");

    let (mut write, mut read) = ws_stream.split();

    let cmd = Command::Subscribe {
        source_id: "topic1".into(),
    };

    write
        .send(tungstenite::protocol::Message::Text(
            serde_json::to_string(&cmd).unwrap(),
        ))
        .await?;

    let resp = read.next().await.expect("Expected response")?;

    let resp: Message<()> = serde_json::from_str(&resp.to_text().unwrap())?;

    match resp {
        Message::CommandResponse(CommandResponse::SubscribeOk { source_id }) => {
            assert_eq!(source_id, "topic1".to_string());
        }
        _ => panic!("Expected subscribe ok"),
    }

    let producer = tokio::spawn(async {
        let producer: FutureProducer = rdkafka::config::ClientConfig::new()
            .set("bootstrap.servers", "localhost:9092")
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
        while let Some(msg) = read.next().await {
            let msg = msg.unwrap();
            let msg: Message<MessageData> = serde_json::from_str(&msg.to_text().unwrap()).unwrap();

            match msg {
                Message::Result(msg) => {
                    assert_eq!(msg.topic.as_ref(), "topic1".to_string());
                    let msg = base64::engine::general_purpose::STANDARD
                        .decode(msg.payload.as_ref().unwrap())
                        .unwrap();
                    assert_eq!(
                        std::str::from_utf8(&msg).unwrap(),
                        format!("Message {}", count)
                    );
                    count += 1;
                }
                _ => panic!("Expected message"),
            }
        }

        assert_eq!(count, 1000, "failed to receive all messages");
    });

    let _ = futures::join!(producer, reader);

    let _ = proc.kill();

    Ok(())
}
