use std::time::Duration;

use anyhow::anyhow;

pub struct Healthcheck<'a> {
    pub interval: Duration,
    pub attempts: u32,
    pub url: &'a str,
}

impl<'a> Healthcheck<'a> {
    pub async fn run(&self) -> anyhow::Result<()> {
        for _ in 0..self.attempts {
            if let Ok(response) = reqwest::get(self.url).await {
                if response.status().is_success() {
                    return Ok(());
                }
            }

            tokio::time::sleep(self.interval).await;
        }

        Err(anyhow!(
            "Healthcheck failed after {} attempts",
            self.attempts
        ))
    }
}
