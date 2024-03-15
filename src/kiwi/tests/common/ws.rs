use fastwebsockets::FragmentCollector;
use fastwebsockets::Frame;
use fastwebsockets::OpCode;
use fastwebsockets::Payload;
use futures::Future;
use http_body_util::Empty;
use tokio::net::TcpStream;

use bytes::Bytes;
use hyper::body::Incoming;
use hyper::header::CONNECTION;
use hyper::header::UPGRADE;
use hyper::upgrade::Upgraded;
use hyper::{Request, Response, Uri};
use hyper_util::rt::TokioIo;

pub struct Client {
    ws: FragmentCollector<TokioIo<Upgraded>>,
}

struct SpawnExecutor;

impl<Fut> hyper::rt::Executor<Fut> for SpawnExecutor
where
    Fut: Future + Send + 'static,
    Fut::Output: Send + 'static,
{
    fn execute(&self, fut: Fut) {
        tokio::task::spawn(fut);
    }
}

impl Client {
    pub async fn connect(uri: &str) -> anyhow::Result<(Self, Response<Incoming>)> {
        let uri: Uri = uri.try_into()?;
        let stream = TcpStream::connect(
            format!("{}:{}", uri.host().unwrap(), uri.port_u16().unwrap()).as_str(),
        )
        .await?;

        let req = Request::builder()
            .method("GET")
            .uri(&uri)
            .header("Host", uri.host().unwrap())
            .header(UPGRADE, "websocket")
            .header(CONNECTION, "upgrade")
            .header(
                "Sec-WebSocket-Key",
                fastwebsockets::handshake::generate_key(),
            )
            .header("Sec-WebSocket-Version", "13")
            .body(Empty::<Bytes>::new())?;

        let (ws, res) = fastwebsockets::handshake::client(&SpawnExecutor, req, stream).await?;

        Ok((
            Self {
                ws: FragmentCollector::new(ws),
            },
            res,
        ))
    }

    pub async fn send_text(&mut self, text: &str) -> anyhow::Result<()> {
        self.ws
            .write_frame(Frame::text(Payload::Borrowed(text.as_bytes())))
            .await?;

        Ok(())
    }

    pub async fn send_json<T: serde::Serialize>(&mut self, value: &T) -> anyhow::Result<()> {
        let text = serde_json::to_string(value)?;
        self.send_text(&text).await?;

        Ok(())
    }

    pub async fn recv_text_frame(&mut self) -> anyhow::Result<Frame<'_>> {
        let frame = self.ws.read_frame().await?;

        match frame.opcode {
            OpCode::Text => Ok(frame),
            _ => Err(anyhow::anyhow!("Expected text frame")),
        }
    }

    pub async fn recv_json<T: serde::de::DeserializeOwned>(&mut self) -> anyhow::Result<T> {
        let text_frame = self.recv_text_frame().await?;
        let text = std::str::from_utf8(text_frame.payload.as_ref())?;
        let value = serde_json::from_str(&text)?;

        Ok(value)
    }
}
