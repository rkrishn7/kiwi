use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;

use futures::stream::select_all::select_all;
use futures::StreamExt;
use thiserror::Error;
use tokio::sync::broadcast::{Receiver, Sender};
use tokio_stream::wrappers::errors::BroadcastStreamRecvError;
use tokio_stream::wrappers::BroadcastStream;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::protocol::CloseFrame;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;

use crate::command::Command;
use crate::topic::TopicBroadcastChannel;

pub struct WSConnection<T> {
    topic_map: Arc<HashMap<String, TopicBroadcastChannel<T>>>,
    /// Subscriptions this connection currently maintains
    subscriptions: HashMap<String, BroadcastStream<T>>,
}

#[derive(Debug)]
/// Represents the current state of the connection task, defining what action
/// it should next take. The states here are externally-driven, meaning external
/// events cause state transitions. As a result, there is no starting state which
/// may depart from the traditional concept of a state machine
enum ConnectionTaskState<T> {
    ProcessingCommand(Command),
    PushingEvent(T),
    Completed,
    // Reporting metrics, etc.
}

#[derive(Debug, Error)]
enum ConnectionError {
    #[error("Unsupported command form. Only UTF-8 encoded text is supported")]
    UnsupportedCommandForm,
    #[error("Encountered an error while deserializing the command payload {0}")]
    CommandDeserialization(String),
}

enum Action<T> {
    Command(Command),
    Event(T),
}

impl<T: Clone + Send + 'static> WSConnection<T> {
    pub fn new(topic_map: Arc<HashMap<String, TopicBroadcastChannel<T>>>) -> Self {
        Self {
            topic_map,
            subscriptions: Default::default(),
        }
    }

    /// Drives this connection to completion by consuming from the specified stream
    pub async fn consume<S>(&mut self, cmd_stream: &mut WebSocketStream<S>) -> anyhow::Result<()>
    where
        S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
    {
        // loop {
        //     let next_state = {
        //         // Combine all the current subscriptions into a single stream
        //         let mut combined = select_all(self.subscriptions.values_mut());

        //         // It's important that we bias the select block towards the command stream
        //         // Because event payload pushes are likely to occur much more frequently than
        //         // commands, we want to ensure the polling of each stream remains fair.
        //         tokio::select! {
        //             biased;

        //             msg = stream.next() => {
        //                 match msg.transpose() {
        //                     Ok(Some(Message::Text(text))) => {
        //                         match serde_json::from_str::<Command>(&text) {
        //                             Ok(cmd) => Ok(ConnectionTaskState::ProcessingCommand(cmd)),
        //                             Err(_) => Err(ConnectionError::CommandDeserialization(text)),
        //                         }
        //                     },
        //                     Ok(None) => Ok(ConnectionTaskState::Completed),
        //                     Ok(_) => Err(ConnectionError::UnsupportedCommandForm),
        //                     Err(e) => {
        //                         tracing::error!("Received error while reading from WS stream: {:?}", e);
        //                         break;
        //                     }
        //                 }
        //             },
        //             event = combined.next() => {
        //                 match event.transpose() {
        //                     Ok(Some(event)) => Action::Event(event),
        //                     Ok(None) => continue,
        //                     Err(e) => {
        //                         match e {
        //                             BroadcastStreamRecvError::Lagged(count) => {
        //                                 tracing::warn!("WS Connection lagged behind by {} messages. Continuing to read from oldest available message", count);
        //                                 continue;
        //                             },
        //                         }
        //                     },
        //                 }
        //             }
        //         }
        //     };

        //     match action {
        //         Action::Command(command) => self.handle_command(command),
        //         Action::Event(_) => todo!(),
        //     }
        // }

        Ok(())
    }

    fn handle_command(&mut self, command: Command) {
        match command {
            Command::Subscribe { topics } => {
                let correct = topics
                    .iter()
                    .all(|topic| self.topic_map.contains_key(topic));

                if correct {
                    for topic in topics.into_iter() {
                        let bm = self.topic_map.get(&topic).expect("known to exist");

                        self.subscriptions
                            .entry(topic)
                            .or_insert(BroadcastStream::new(bm.subscribe()));
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
