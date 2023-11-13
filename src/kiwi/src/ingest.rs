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
use crate::protocol::{Command, CommandResponse, Message, Notice};
use crate::source::{Source, SourceId};

/// This actor is responsible for the following tasks:
/// - Processing commands as they become available
/// - Reading events from subscribed sources, processing them, and
///   forwarding them along its active subscriptions
pub struct IngestActor<S, T, M, P> {
    cmd_rx: UnboundedReceiver<Command>,
    msg_tx: UnboundedSender<Message<M>>,
    shutdown_tripwire: Fuse<oneshot::Receiver<()>>,
    sources: Arc<BTreeMap<SourceId, S>>,
    /// Subscriptions this actor currently maintains for its handle
    subscriptions: HashMap<SourceId, BroadcastStream<T>>,
    connection_ctx: plugin::types::ConnectionCtx,
    pre_forward: Option<P>,
}

#[derive(Debug)]
/// Represents the current state of the actor's main processing loop, defining what action
/// it should next take. The states here are externally-driven, meaning external
/// events cause state transitions. As a result, there is no starting state which
/// may depart from the traditional concept of a state machine
enum IngestActorState<T> {
    ReceivedCommand(Command),
    ReceivedSourceEvent(T),
    Lagged((SourceId, u64)),
}

impl<S, T, M, P> IngestActor<S, T, M, P>
where
    M: Clone + Send + Sync + 'static,
    S: Source<Message = T>,
    T: Into<plugin::types::EventCtx> + Into<M> + MutableEvent + Debug + Clone + Send + 'static,
    P: Plugin + Clone + Send + 'static,
{
    pub fn new(
        sources: Arc<BTreeMap<String, S>>,
        cmd_rx: UnboundedReceiver<Command>,
        msg_tx: UnboundedSender<Message<M>>,
        connection_ctx: plugin::types::ConnectionCtx,
        pre_forward: Option<P>,
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
                let mut combined =
                    select_all(self.subscriptions.iter_mut().map(|(source_id, stream)| {
                        crate::util::stream::with_id(source_id, stream)
                    }));

                tokio::select! {
                    biased;

                    _ = &mut self.shutdown_tripwire => break,
                    maybe_cmd = self.cmd_rx.recv() => {
                        match maybe_cmd {
                            Some(cmd) => IngestActorState::ReceivedCommand(cmd),
                            None => break,
                        }
                    },
                    event = combined.next() => {
                        match event {
                            Some((source_id, res)) => {
                                match res {
                                    Ok(event) => IngestActorState::ReceivedSourceEvent(event),
                                    Err(BroadcastStreamRecvError::Lagged(count)) => IngestActorState::Lagged((source_id.clone(), count))
                                }
                            },
                            // Since the stream combinator is re-computed on each iteration, receiving
                            // `None` does not signal we are done. It is very possible that the actor
                            // handle later signals to add a new subscription via `cmd_tx`
                            None => continue,
                        }
                    }
                }
            };

            match next_state {
                IngestActorState::ReceivedCommand(cmd) => {
                    // TODO: we should probably abort the connection if we fail to handle a command
                    let _ = self.handle_command(cmd);
                }
                IngestActorState::ReceivedSourceEvent(event) => {
                    if let Err(_) = self.forward_event(event).await {
                        tracing::error!("Error while forwarding source event");
                    }
                }
                IngestActorState::Lagged((source_id, count)) => {
                    tracing::warn!("Actor lagged behind by {} messages for source {}. Continuing to read from oldest available message", count, source_id);

                    // If we fail to send the message, it means the receiving half of the message channel
                    // was dropped, in which case we want to complete execution
                    if let Err(_) = self.msg_tx.send(Message::Notice(Notice::Lag {
                        source: source_id,
                        count,
                    })) {
                        break;
                    }
                }
            }
        }

        Ok(())
    }

    fn handle_command(&mut self, command: Command) -> anyhow::Result<()> {
        match command {
            Command::Subscribe { sources } => {
                let sources_exist = sources.iter().all(|id| self.sources.contains_key(id));

                if sources_exist {
                    for source_id in sources.iter() {
                        let source = self.sources.get(source_id).expect("known to exist");

                        self.subscriptions
                            .entry(source_id.clone())
                            .or_insert(BroadcastStream::new(source.subscribe()));
                    }

                    self.msg_tx
                        .send(Message::CommandResponse(CommandResponse::SubscribeOk {
                            sources,
                        }))?;
                } else {
                    self.msg_tx.send(Message::CommandResponse(
                        CommandResponse::SubscribeError {
                            sources,
                            error: "One or more source identifiers do not exist on this server"
                                .to_string(),
                        },
                    ))?;
                }
            }
            Command::Unsubscribe { sources } => {
                let sources_exist = sources.iter().all(|id| self.sources.contains_key(id));

                if sources_exist {
                    for source_id in sources.iter() {
                        let _ = self.subscriptions.remove(source_id);
                    }

                    self.msg_tx
                        .send(Message::CommandResponse(CommandResponse::UnsubscribeOk {
                            sources,
                        }))?;
                } else {
                    self.msg_tx.send(Message::CommandResponse(
                        CommandResponse::UnsubscribeError {
                            sources,
                            error: "One or more source identifiers do not exist on this server"
                                .to_string(),
                        },
                    ))?;
                }
            }
        }

        Ok(())
    }

    async fn forward_event(&mut self, event: T) -> anyhow::Result<()> {
        let plugin_event_ctx: plugin::types::EventCtx = event.clone().into();
        let plugin_ctx = plugin::types::Context {
            connection: self.connection_ctx.clone(),
            event: plugin_event_ctx,
        };

        let action = if let Some(plugin) = self.pre_forward.clone() {
            let result = tokio::task::spawn_blocking(move || plugin.call(&plugin_ctx)).await??;

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
