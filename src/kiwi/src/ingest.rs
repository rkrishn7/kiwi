use std::collections::{BTreeMap, HashMap};
use std::fmt::Debug;
use std::sync::Arc;

use futures::future::Fuse;
use futures::stream::select_all::select_all;
use futures::StreamExt;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio::sync::oneshot;
use tokio_stream::wrappers::errors::BroadcastStreamRecvError;
use tokio_stream::wrappers::BroadcastStream;

use crate::event::MutableEvent;
use crate::plugin::{self, Plugin};
use crate::protocol::{Command, Message, Notice};
use crate::source::Source;

/// This actor is responsible for the following tasks:
/// - Processing commands as they become available
/// - Reading events from subscribed topics, processing them, and
///   forwarding them along the specified outbound message channel
pub struct IngestActor<S, T, M> {
    cmd_rx: UnboundedReceiver<Command>,
    msg_tx: UnboundedSender<Message<M>>,
    shutdown_tripwire: Fuse<oneshot::Receiver<()>>,
    sources: Arc<BTreeMap<String, S>>,
    /// Subscriptions this actor currently maintains for its handle
    subscriptions: HashMap<String, BroadcastStream<T>>,
    connection_ctx: plugin::types::ConnectionCtx,
    pre_forward: Option<Plugin>,
}

#[derive(Debug)]
/// Represents the current state of the actor's main processing loop, defining what action
/// it should next take. The states here are externally-driven, meaning external
/// events cause state transitions. As a result, there is no starting state which
/// may depart from the traditional concept of a state machine
enum IngestActorState<T> {
    ProcessingCommand(Command),
    ProcessingTopicEvent(T),
    ProcessingLag(u64),
}

impl<S, T, M> IngestActor<S, T, M>
where
    M: Clone + Send + Sync + 'static,
    S: Source<Message = T>,
    T: Into<plugin::types::EventCtx> + Into<M> + MutableEvent + Debug + Clone + Send + 'static,
{
    pub fn new(
        sources: Arc<BTreeMap<String, S>>,
        cmd_rx: UnboundedReceiver<Command>,
        msg_tx: UnboundedSender<Message<M>>,
        connection_ctx: plugin::types::ConnectionCtx,
        pre_forward: Option<Plugin>,
        shutdown_tripwire: Fuse<oneshot::Receiver<()>>,
    ) -> Self {
        Self {
            cmd_rx,
            msg_tx,
            shutdown_tripwire,
            sources,
            connection_ctx,
            subscriptions: Default::default(),
            pre_forward,
        }
    }

    /// Drives this connection to completion by consuming from the specified stream
    pub async fn run(mut self) -> anyhow::Result<()> {
        loop {
            let next_state = {
                // Combine all the current subscriptions into a single stream
                let mut combined = select_all(self.subscriptions.values_mut());

                tokio::select! {
                    biased;

                    _ = &mut self.shutdown_tripwire => break,
                    maybe_cmd = self.cmd_rx.recv() => {
                        match maybe_cmd {
                            Some(cmd) => IngestActorState::ProcessingCommand(cmd),
                            None => break,
                        }
                    },
                    event = combined.next() => {
                        match event.transpose() {
                            Ok(Some(event)) => IngestActorState::ProcessingTopicEvent(event),
                            // Since the stream combinator is re-computed on each iteration, receiving
                            // `None` does not signal we are done. It is very possible that the actor
                            // handle later signals to add a new subscription via `cmd_tx`
                            Ok(None) => continue,
                            Err(BroadcastStreamRecvError::Lagged(count)) => IngestActorState::ProcessingLag(count),
                        }
                    }
                }
            };

            match next_state {
                IngestActorState::ProcessingCommand(cmd) => {
                    self.handle_command(cmd);
                }
                IngestActorState::ProcessingTopicEvent(event) => {
                    if let Err(_) = self.forward_event(event).await {
                        tracing::error!("Error while forwarding topic event");
                    }
                }
                IngestActorState::ProcessingLag(count) => {
                    tracing::warn!("Actor lagged behind by {} messages. Continuing to read from oldest available message", count);
                    let topics = self.subscriptions.keys().cloned().collect::<Vec<_>>();

                    // If we fail to send the message, it means the receiving half of the message channel
                    // was dropped, in which case we want to complete execution
                    if let Err(_) = self
                        .msg_tx
                        .send(Message::Notice(Notice::Lag { topics, count }))
                    {
                        break;
                    }
                }
            }
        }

        Ok(())
    }

    fn handle_command(&mut self, command: Command) {
        match command {
            Command::Subscribe { topics } => {
                let correct = topics.iter().all(|topic| self.sources.contains_key(topic));

                if correct {
                    for topic in topics.into_iter() {
                        let source = self.sources.get(&topic).expect("known to exist");

                        self.subscriptions
                            .entry(topic)
                            .or_insert(BroadcastStream::new(source.subscribe()));
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

    async fn forward_event(&mut self, event: T) -> anyhow::Result<()> {
        let plugin_event_ctx: plugin::types::EventCtx = event.clone().into();
        let plugin_ctx = plugin::types::Context {
            connection: self.connection_ctx.clone(),
            event: plugin_event_ctx,
        };

        let action = if let Some(plugin) = self.pre_forward.clone() {
            let result = tokio::task::spawn_blocking(move || plugin.call(plugin_ctx)).await??;

            Some(result)
        } else {
            None
        };

        match action {
            Some(plugin::types::Action::Discard) => (),
            None | Some(plugin::types::Action::Forward) => {
                self.msg_tx.send(Message::Result(event.into()))?;
            }
            Some(plugin::types::Action::Transform(payload)) => {
                let transformed = event.set_payload(payload);

                self.msg_tx.send(Message::Result(transformed.into()))?;
            }
        }

        Ok(())
    }
}
