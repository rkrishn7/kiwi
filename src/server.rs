use std::{collections::HashMap, net::SocketAddr, sync::Arc};
use tokio::net::TcpListener;

use crate::{topic::TopicBroadcastChannel, ws::WSConnection};

pub async fn serve<T>(
    listen_addr: &SocketAddr,
    topic_broadcast_map: Arc<HashMap<String, TopicBroadcastChannel<T>>>,
) -> anyhow::Result<()>
where
    T: Clone + Send + 'static,
{
    let listener = TcpListener::bind(listen_addr).await?;
    tracing::info!("Server started. Listening on: {}", listen_addr);

    while let Ok((stream, addr)) = listener.accept().await {
        let topic_broadcast_map = Arc::clone(&topic_broadcast_map);
        tokio::spawn(async move {
            match tokio_tungstenite::accept_async(stream).await {
                Ok(mut ws_stream) => {
                    tracing::info!("New WebSocket connection: {:?}", ws_stream);

                    let mut connection = WSConnection::<T>::new(topic_broadcast_map);

                    let _ = connection.consume(&mut ws_stream).await;
                }
                Err(err) => {
                    tracing::error!(
                        "Error during websocket handshake occurred for client {}: {}",
                        addr,
                        err
                    );
                }
            }
        });
    }

    todo!()
}
