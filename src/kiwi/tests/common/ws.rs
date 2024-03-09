use futures::{SinkExt, StreamExt};
use http::Response;
use tokio::net::TcpStream;
use tokio_tungstenite::{
    connect_async,
    tungstenite::{client::IntoClientRequest, Message},
    MaybeTlsStream, WebSocketStream,
};

pub struct Client {
    stream: WebSocketStream<MaybeTlsStream<TcpStream>>,
}

impl Client {
    pub async fn connect<R: IntoClientRequest + Unpin>(
        url: R,
    ) -> anyhow::Result<(Self, Response<Option<Vec<u8>>>)> {
        let (stream, response) = connect_async(url).await?;

        Ok((Self { stream }, response))
    }

    pub async fn send_text(&mut self, text: &str) -> anyhow::Result<()> {
        self.stream.send(Message::Text(text.to_string())).await?;

        Ok(())
    }

    pub async fn send_json<T: serde::Serialize>(&mut self, value: &T) -> anyhow::Result<()> {
        let text = serde_json::to_string(value)?;
        self.send_text(&text).await?;

        Ok(())
    }

    pub async fn recv(&mut self) -> Option<Result<Message, tokio_tungstenite::tungstenite::Error>> {
        self.stream.next().await
    }

    pub async fn recv_json<T: serde::de::DeserializeOwned>(&mut self) -> anyhow::Result<T> {
        let message = self
            .recv()
            .await
            .ok_or_else(|| anyhow::anyhow!("Connection closed"))??;
        let text = message.to_text()?;
        let value = serde_json::from_str(&text)?;

        Ok(value)
    }
}
