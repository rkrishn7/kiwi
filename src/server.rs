use rdkafka::message::OwnedMessage;
use std::{collections::HashMap, net::SocketAddr, sync::Arc};
use tokio::net::TcpListener;

use crate::ws::WSConnection;

pub async fn serve(listen_addr: &SocketAddr) -> anyhow::Result<()> {
    let listener = TcpListener::bind(listen_addr).await?;
    tracing::info!("Server started. Listening on: {}", listen_addr);

    while let Ok((stream, _)) = listener.accept().await {
        tokio::spawn(async {
            match tokio_tungstenite::accept_async(stream).await {
                Ok(ws_stream) => {
                    tracing::info!("New WebSocket connection: {:?}", ws_stream);

                    let connection = WSConnection::<OwnedMessage>::new(Arc::new(HashMap::new()));

                    connection.consume(ws_stream).await;
                }
                Err(err) => {
                    tracing::error!("Error during the websocket handshake occurred: {}", err);
                }
            }
        });
    }

    todo!()
}
