mod common;

use std::time::Duration;

use common::kiwi::{ConfigFile, Process};
use nix::sys::signal;

static GRACE_PERIOD_MS: u64 = 500;

#[tokio::test]
async fn test_shuts_down_sigint() -> anyhow::Result<()> {
    let config = ConfigFile::from_str(
        format!(
            r#"
        sources: []
        server:
            address: '127.0.0.1:8000'
        "#
        )
        .as_str(),
    )?;

    let mut kiwi = Process::new_with_args(&["--config", config.path_str()])?;

    kiwi.signal(signal::SIGINT)?;

    tokio::time::sleep(Duration::from_millis(GRACE_PERIOD_MS)).await;

    let status = kiwi.proc_mut().try_wait()?;

    assert!(status.is_some());

    Ok(())
}

#[tokio::test]
async fn test_shuts_down_sigterm() -> anyhow::Result<()> {
    let config = ConfigFile::from_str(
        format!(
            r#"
        sources: []
        server:
            address: '127.0.0.1:8000'
        "#
        )
        .as_str(),
    )?;

    let mut kiwi = Process::new_with_args(&["--config", config.path_str()])?;

    kiwi.signal(signal::SIGTERM)?;

    tokio::time::sleep(Duration::from_millis(GRACE_PERIOD_MS)).await;

    let status = kiwi.proc_mut().try_wait()?;

    assert!(status.is_some());

    Ok(())
}
