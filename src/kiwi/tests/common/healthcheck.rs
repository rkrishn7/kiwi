use std::{future::IntoFuture, time::Duration};

use anyhow::anyhow;
use futures::Future;

pub struct Healthcheck<'a> {
    pub interval: Duration,
    pub attempts: u32,
    pub url: &'a str,
}

impl<'a> Healthcheck<'a> {
    async fn run(&self) -> anyhow::Result<()> {
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

impl<'a> Future for Healthcheck<'a> {
    type Output = anyhow::Result<()>;

    fn poll(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        let fut = self.run().into_future();

        let mut pinned_fut = std::pin::pin!(fut);

        pinned_fut.as_mut().poll(cx)
    }
}
