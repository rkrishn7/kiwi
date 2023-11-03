use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;

use futures::stream::select_all::select_all;
use futures::StreamExt;
use tokio::sync::broadcast::{Receiver, Sender};
use tokio_stream::wrappers::errors::BroadcastStreamRecvError;
use tokio_stream::wrappers::BroadcastStream;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::protocol::CloseFrame;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;

use crate::command::Command;

pub struct WSConnection<T> {
    topic_map: Arc<HashMap<String, (Sender<T>, Receiver<T>)>>,
    /// Subscriptions this connection currently maintains
    subscriptions: HashMap<String, BroadcastStream<T>>,
}

enum Action<T> {
    Command(Command),
    Event(T),
}

impl<T: Clone + Send + 'static> WSConnection<T> {
    pub fn new(topic_map: Arc<HashMap<String, (Sender<T>, Receiver<T>)>>) -> Self {
        Self {
            topic_map,
            subscriptions: Default::default(),
        }
    }

    /// TODO: return result here
    pub async fn consume<S>(mut self, mut stream: WebSocketStream<S>)
    where
        S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
    {
        loop {
            let action = {
                // Combine all the current subscriptions into a single stream
                let mut combined = select_all(self.subscriptions.values_mut());

                // It's important that we bias the select block towards the WebSocket stream
                // in order to prioritize commands from the client. Because event payload pushes
                // are likely to occur much more frequently than commands, we want to ensure the
                // polling of each stream remains fair.
                tokio::select! {
                    biased;

                    msg = stream.next() => {
                        match msg.transpose() {
                            Ok(Some(Message::Text(text))) => {
                                match serde_json::from_str::<Command>(&text) {
                                    Ok(cmd) => Action::Command(cmd),
                                    Err(_) => {
                                        let close_frame = CloseFrame {
                                            code: CloseCode::Protocol,
                                            reason: Cow::from("Received unknown command"),
                                        };
                                        stream.close(Some(close_frame)).await.unwrap();
                                        break;
                                    },
                                }
                            },
                            Ok(None) => {
                                break;
                            },
                            Ok(_) => {
                                // If we receive a non-textual message, we should close the
                                // connection with an appropriate error code.

                                let close_frame = CloseFrame {
                                    code: CloseCode::Unsupported,
                                    reason: Cow::from("Unsupported message form. Only UTF-8 encoded text is supported"),
                                };
                                stream.close(Some(close_frame)).await.unwrap();
                                break;
                            },
                            Err(e) => {
                                tracing::error!("Received error while reading from WS stream: {:?}", e);
                                break;
                            }
                        }
                    },
                    event = combined.next() => {
                        match event.transpose() {
                            Ok(Some(event)) => Action::Event(event),
                            Ok(None) => break,
                            Err(e) => {
                                match e {
                                    BroadcastStreamRecvError::Lagged(count) => {
                                        tracing::warn!("WS Connection lagged behind by {} messages. Continuing to read from oldest available message", count);
                                        continue;
                                    },
                                }
                            },
                        }
                    }
                }
            };

            match action {
                Action::Command(command) => self.handle_command(command),
                Action::Event(_) => todo!(),
            }
        }
    }

    fn handle_command(&mut self, command: Command) {
        match command {
            Command::Subscribe { topics } => {
                let correct = topics
                    .iter()
                    .all(|topic| self.topic_map.contains_key(topic));

                if correct {
                    for topic in topics.into_iter() {
                        let (tx, _) = self.topic_map.get(&topic).expect("known to exist");

                        self.subscriptions
                            .entry(topic)
                            .or_insert(BroadcastStream::new(tx.subscribe()));
                    }
                } else {
                    // Send error response
                }
            }
            Command::Unsubscribe { topics } => {
                for topic in topics.into_iter() {
                    let _ = self.subscriptions.remove(&topic);
                }
            }
        }
    }
}
