pub mod common;

use std::time::Duration;

use common::kiwi::{ConfigFile, Process};

use crate::common::healthcheck::Healthcheck;
use crate::common::ws::Client as WsClient;

/// Test that the `authenticate` hook works as expected.
#[tokio::test]
async fn test_authenticate() -> anyhow::Result<()> {
    const AUTHENTICATE_PATH: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/wasm/authenticate-api-key.wasm"
    );

    let config = ConfigFile::from_str(
        format!(
            r#"
        hooks:
            authenticate: {AUTHENTICATE_PATH}
        sources: []
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

    assert!(WsClient::connect("ws://127.0.0.1:8000").await.is_err());
    assert!(WsClient::connect("ws://127.0.0.1:8000/?x-api-key=wrong")
        .await
        .is_err());
    assert!(WsClient::connect("ws://127.0.0.1:8000/?x-api-key=12345")
        .await
        .is_ok());

    Ok(())
}
