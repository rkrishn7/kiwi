use std::collections::{BTreeMap, HashMap};
use std::fmt::Debug;
use std::sync::Arc;

use futures::stream::select_all::select_all;
use futures::StreamExt;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
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
    ) -> Self {
        Self {
            cmd_rx,
            msg_tx,
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

                    maybe_cmd = self.cmd_rx.recv() => {
                        match maybe_cmd {
                            Some(cmd) => IngestActorState::ReceivedCommand(cmd),
                            None => break,
                        }
                    },
                    // Since the stream combinator is re-computed on each iteration, receiving
                    // `None` does not signal we are done. It is very possible that the actor
                    // handle later signals to add a new subscription via `cmd_tx`
                    Some((source_id, res)) = combined.next() => {
                        match res {
                            Ok(event) => IngestActorState::ReceivedSourceEvent(event),
                            Err(BroadcastStreamRecvError::Lagged(count)) => IngestActorState::Lagged((source_id.clone(), count))
                        }
                    },
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

                // Ensure none of the subscriptions already exist
                let subscriptions_absent = sources
                    .iter()
                    .all(|id| self.subscriptions.get(id).is_none());

                if !subscriptions_absent {
                    self.msg_tx.send(Message::CommandResponse(
                        CommandResponse::SubscribeError {
                            sources,
                            error: "One or more requested sources are already subscribed to"
                                .to_string(),
                        },
                    ))?;
                } else if !sources_exist {
                    self.msg_tx.send(Message::CommandResponse(
                        CommandResponse::SubscribeError {
                            sources,
                            error: "One or more source identifiers do not exist on this server"
                                .to_string(),
                        },
                    ))?;
                } else {
                    for source_id in sources.iter() {
                        let source = self.sources.get(source_id).expect("known to exist");

                        // We know the subscription does not exist by this point
                        self.subscriptions
                            .entry(source_id.clone())
                            .or_insert(BroadcastStream::new(source.subscribe()));
                    }

                    self.msg_tx
                        .send(Message::CommandResponse(CommandResponse::SubscribeOk {
                            sources,
                        }))?;
                }
            }
            Command::Unsubscribe { sources } => {
                let sources_exist = sources.iter().all(|id| self.sources.contains_key(id));
                // Ensure none of the subscriptions already exist
                let subscriptions_present = sources
                    .iter()
                    .all(|id| self.subscriptions.get(id).is_some());

                if !subscriptions_present {
                    self.msg_tx.send(Message::CommandResponse(
                        CommandResponse::UnsubscribeError {
                            sources,
                            error: "One or more requested sources are not subscribed to"
                                .to_string(),
                        },
                    ))?;
                } else if !sources_exist {
                    self.msg_tx.send(Message::CommandResponse(
                        CommandResponse::UnsubscribeError {
                            sources,
                            error: "One or more source identifiers do not exist on this server"
                                .to_string(),
                        },
                    ))?;
                } else {
                    for source_id in sources.iter() {
                        let _ = self.subscriptions.remove(source_id);
                    }

                    self.msg_tx
                        .send(Message::CommandResponse(CommandResponse::UnsubscribeOk {
                            sources,
                        }))?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::sync::broadcast::{Receiver, Sender};

    #[derive(Debug, Clone)]
    struct TestSource {
        tx: Sender<TestMessage>,
        source_id: SourceId,
    }

    #[derive(Debug, Clone)]
    struct TestMessage {
        payload: Option<Vec<u8>>,
    }

    impl From<TestMessage> for plugin::types::EventCtx {
        fn from(value: TestMessage) -> Self {
            Self::Kafka(plugin::types::KafkaEventCtx {
                payload: value.payload,
                topic: "test".to_string(),
                timestamp: None,
                partition: 0,
                offset: 0,
            })
        }
    }

    impl MutableEvent for TestMessage {
        fn set_payload(mut self, payload: Option<Vec<u8>>) -> Self {
            self.payload = payload;
            self
        }
    }

    impl Source for TestSource {
        type Message = TestMessage;

        fn subscribe(&self) -> Receiver<Self::Message> {
            self.tx.subscribe()
        }

        fn source_id(&self) -> &SourceId {
            &self.source_id
        }
    }

    #[derive(Debug, Clone)]
    /// Discards all events
    struct DiscardPlugin;

    impl Plugin for DiscardPlugin {
        fn call(&self, _ctx: &plugin::types::Context) -> anyhow::Result<plugin::types::Action> {
            Ok(plugin::types::Action::Discard)
        }
    }

    #[tokio::test]
    async fn test_actor_completes_on_cmd_rx_drop() {
        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel::<Command>();
        let (msg_tx, _) = tokio::sync::mpsc::unbounded_channel::<Message<TestMessage>>();

        let (tx, _) = tokio::sync::broadcast::channel::<TestMessage>(100);

        let test_source = TestSource {
            tx,
            source_id: "test".to_string(),
        };

        let mut sources = BTreeMap::new();
        sources.insert(test_source.source_id().clone(), test_source);

        let actor = IngestActor::new(
            Arc::new(sources),
            cmd_rx,
            msg_tx,
            plugin::types::ConnectionCtx::WebSocket(plugin::types::WebSocketConnectionCtx {
                auth: None,
                addr: "127.0.0.1:8000".parse().unwrap(),
            }),
            Some(DiscardPlugin),
        );

        let actor_handle = tokio::spawn(actor.run());

        // Drop the command channel, which should cause the actor to complete
        drop(cmd_tx);

        assert!(
            actor_handle.await.unwrap().is_ok(),
            "ingest actor should complete successfully when command channel is closed"
        );
    }

    #[tokio::test]
    async fn test_source_subscribing() {
        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel::<Command>();
        let (msg_tx, mut msg_rx) = tokio::sync::mpsc::unbounded_channel::<Message<TestMessage>>();

        let (tx, _) = tokio::sync::broadcast::channel::<TestMessage>(100);

        let test_source = TestSource {
            tx,
            source_id: "test".to_string(),
        };

        let mut sources = BTreeMap::new();
        sources.insert(test_source.source_id().clone(), test_source);

        let actor = IngestActor::new(
            Arc::new(sources),
            cmd_rx,
            msg_tx,
            plugin::types::ConnectionCtx::WebSocket(plugin::types::WebSocketConnectionCtx {
                auth: None,
                addr: "127.0.0.1:8000".parse().unwrap(),
            }),
            Some(DiscardPlugin),
        );

        tokio::spawn(actor.run());

        cmd_tx
            .send(Command::Subscribe {
                sources: vec!["test".to_string()],
            })
            .unwrap();

        // Ensure the actor responds with a subscribe ok message
        let msg = msg_rx.recv().await.unwrap();
        match msg {
            Message::CommandResponse(CommandResponse::SubscribeOk { sources })
                if sources == vec!["test"] =>
            {
                ()
            }
            _ => panic!("actor should respond with correct subscribe ok message"),
        }

        // Ensure resubscribing to the same source results in an error
        cmd_tx
            .send(Command::Subscribe {
                sources: vec!["test".to_string()],
            })
            .unwrap();
        let msg = msg_rx.recv().await.unwrap();
        assert!(
            matches!(
                msg,
                Message::CommandResponse(CommandResponse::SubscribeError { .. })
            ),
            "re-subscribing to the same source should result in an error"
        );

        // Ensure subscribing to a non-existent source results in an error
        cmd_tx
            .send(Command::Subscribe {
                sources: vec!["test2".to_string()],
            })
            .unwrap();
        let msg = msg_rx.recv().await.unwrap();
        assert!(
            matches!(
                msg,
                Message::CommandResponse(CommandResponse::SubscribeError { .. })
            ),
            "subscribing to a non-existent source should result in an error"
        );
    }

    #[tokio::test]
    async fn test_source_unsubscribing() {
        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel::<Command>();
        let (msg_tx, mut msg_rx) = tokio::sync::mpsc::unbounded_channel::<Message<TestMessage>>();

        let (tx, _) = tokio::sync::broadcast::channel::<TestMessage>(100);

        let test_source = TestSource {
            tx,
            source_id: "test".to_string(),
        };

        let mut sources = BTreeMap::new();
        sources.insert(test_source.source_id().clone(), test_source);

        let actor = IngestActor::new(
            Arc::new(sources),
            cmd_rx,
            msg_tx,
            plugin::types::ConnectionCtx::WebSocket(plugin::types::WebSocketConnectionCtx {
                auth: None,
                addr: "127.0.0.1:8000".parse().unwrap(),
            }),
            Some(DiscardPlugin),
        );

        tokio::spawn(actor.run());

        // Check that unsubscribing from a non-existent subscription results in an error
        cmd_tx
            .send(Command::Unsubscribe {
                sources: vec!["test".to_string()],
            })
            .unwrap();

        match msg_rx.recv().await.unwrap() {
            Message::CommandResponse(CommandResponse::UnsubscribeError { sources, .. })
                if sources == vec!["test"] =>
            {
                ()
            }
            m => panic!(
                "actor should respond with unsubscribe error message. Instead responded with {:?}",
                m
            ),
        }

        cmd_tx
            .send(Command::Subscribe {
                sources: vec!["test".to_string()],
            })
            .unwrap();

        // Ensure the actor responds with a subscribe ok message
        match msg_rx.recv().await.unwrap() {
            Message::CommandResponse(CommandResponse::SubscribeOk { sources })
                if sources == vec!["test"] =>
            {
                ()
            }
            _ => panic!("actor should respond with correct subscribe ok message"),
        }

        // Ensure that we can unsubscribe from an existing subscription
        cmd_tx
            .send(Command::Unsubscribe {
                sources: vec!["test".to_string()],
            })
            .unwrap();
        assert!(
            matches!(
                msg_rx.recv().await.unwrap(),
                Message::CommandResponse(CommandResponse::UnsubscribeOk { .. })
            ),
            "unsubscribing from an existing subscription should work"
        );
    }

    #[tokio::test]
    async fn test_plugin_discard_action() {
        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel::<Command>();
        let (msg_tx, mut msg_rx) = tokio::sync::mpsc::unbounded_channel::<Message<TestMessage>>();

        let (tx, _) = tokio::sync::broadcast::channel::<TestMessage>(100);

        let test_source = TestSource {
            tx: tx.clone(),
            source_id: "test".to_string(),
        };

        let mut sources = BTreeMap::new();
        sources.insert(test_source.source_id().clone(), test_source);

        let actor = IngestActor::new(
            Arc::new(sources),
            cmd_rx,
            msg_tx,
            plugin::types::ConnectionCtx::WebSocket(plugin::types::WebSocketConnectionCtx {
                auth: None,
                addr: "127.0.0.1:8000".parse().unwrap(),
            }),
            Some(DiscardPlugin),
        );

        tokio::spawn(actor.run());

        cmd_tx
            .send(Command::Subscribe {
                sources: vec!["test".to_string()],
            })
            .unwrap();

        // Ensure the actor responds with a subscribe ok message
        let msg = msg_rx.recv().await.unwrap();
        match msg {
            Message::CommandResponse(CommandResponse::SubscribeOk { sources })
                if sources == vec!["test"] =>
            {
                ()
            }
            _ => panic!("actor should respond with correct subscribe ok message"),
        }

        for _ in 0..10 {
            tx.send(TestMessage { payload: None }).unwrap();
        }

        // TODO: Is there a better way to ensure the actor does not forward any messages?
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_millis(500)) => (),
            _ = msg_rx.recv() => panic!("actor should not forward any messages when discard action is returned")
        }
    }

    #[tokio::test]
    async fn test_plugin_forward_action() {
        #[derive(Debug, Clone)]
        struct ForwardPlugin;

        impl Plugin for ForwardPlugin {
            fn call(&self, _ctx: &plugin::types::Context) -> anyhow::Result<plugin::types::Action> {
                Ok(plugin::types::Action::Forward)
            }
        }

        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel::<Command>();
        let (msg_tx, mut msg_rx) = tokio::sync::mpsc::unbounded_channel::<Message<TestMessage>>();

        let (tx, _) = tokio::sync::broadcast::channel::<TestMessage>(100);

        let test_source = TestSource {
            tx: tx.clone(),
            source_id: "test".to_string(),
        };

        let mut sources = BTreeMap::new();
        sources.insert(test_source.source_id().clone(), test_source);

        let actor = IngestActor::new(
            Arc::new(sources),
            cmd_rx,
            msg_tx,
            plugin::types::ConnectionCtx::WebSocket(plugin::types::WebSocketConnectionCtx {
                auth: None,
                addr: "127.0.0.1:8000".parse().unwrap(),
            }),
            Some(ForwardPlugin),
        );

        tokio::spawn(actor.run());

        cmd_tx
            .send(Command::Subscribe {
                sources: vec!["test".to_string()],
            })
            .unwrap();

        // Ensure the actor responds with a subscribe ok message
        let msg = msg_rx.recv().await.unwrap();
        match msg {
            Message::CommandResponse(CommandResponse::SubscribeOk { sources })
                if sources == vec!["test"] =>
            {
                ()
            }
            _ => panic!("actor should respond with correct subscribe ok message"),
        }

        for _ in 0..10 {
            tx.send(TestMessage { payload: None }).unwrap();
        }

        let received_all_messages = {
            for _ in 0..10 {
                let msg = msg_rx.recv().await.unwrap();
                match msg {
                    Message::Result(_) => (),
                    _ => panic!("actor should forward message when forward action is returned"),
                }
            }
            true
        };

        assert!(received_all_messages, "actor should forward all messages");
    }

    #[tokio::test]
    async fn test_plugin_transform_action() {
        #[derive(Debug, Clone)]
        struct TransformPlugin;

        impl Plugin for TransformPlugin {
            fn call(&self, _ctx: &plugin::types::Context) -> anyhow::Result<plugin::types::Action> {
                Ok(plugin::types::Action::Transform(Some(
                    "hello".as_bytes().to_owned(),
                )))
            }
        }

        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel::<Command>();
        let (msg_tx, mut msg_rx) = tokio::sync::mpsc::unbounded_channel::<Message<TestMessage>>();

        let (tx, _) = tokio::sync::broadcast::channel::<TestMessage>(100);

        let test_source = TestSource {
            tx: tx.clone(),
            source_id: "test".to_string(),
        };

        let mut sources = BTreeMap::new();
        sources.insert(test_source.source_id().clone(), test_source);

        let actor = IngestActor::new(
            Arc::new(sources),
            cmd_rx,
            msg_tx,
            plugin::types::ConnectionCtx::WebSocket(plugin::types::WebSocketConnectionCtx {
                auth: None,
                addr: "127.0.0.1:8000".parse().unwrap(),
            }),
            Some(TransformPlugin),
        );

        tokio::spawn(actor.run());

        cmd_tx
            .send(Command::Subscribe {
                sources: vec!["test".to_string()],
            })
            .unwrap();

        // Ensure the actor responds with a subscribe ok message
        let msg = msg_rx.recv().await.unwrap();
        match msg {
            Message::CommandResponse(CommandResponse::SubscribeOk { sources })
                if sources == vec!["test"] =>
            {
                ()
            }
            _ => panic!("actor should respond with correct subscribe ok message"),
        }

        for _ in 0..10 {
            tx.send(TestMessage { payload: None }).unwrap();
        }

        let received_all_messages = {
            for _ in 0..10 {
                let msg = msg_rx.recv().await.unwrap();
                match msg {
                    Message::Result(m) => {
                        assert_eq!(
                            m.payload,
                            Some("hello".as_bytes().to_owned()),
                            "message payload should have been transformed"
                        );
                    }
                    _ => panic!("actor should forward message when forward action is returned"),
                }
            }
            true
        };

        assert!(received_all_messages, "actor should forward all messages");
    }

    #[tokio::test]
    async fn test_lag_notices() {
        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel::<Command>();
        let (msg_tx, mut msg_rx) = tokio::sync::mpsc::unbounded_channel::<Message<TestMessage>>();

        let (tx, _) = tokio::sync::broadcast::channel::<TestMessage>(100);

        let tx2 = tx.clone();

        let test_source = TestSource {
            tx,
            source_id: "test".to_string(),
        };

        #[derive(Debug, Clone)]
        /// Simulates a slow executing plugin
        struct SlowPlugin;

        // The bottleneck for the actor is now this plugin. Since the actor calls it
        // synchronously, it can only process around 10 messages/sec
        impl Plugin for SlowPlugin {
            fn call(&self, _ctx: &plugin::types::Context) -> anyhow::Result<plugin::types::Action> {
                std::thread::sleep(Duration::from_millis(100));
                Ok(plugin::types::Action::Forward)
            }
        }

        let mut sources = BTreeMap::new();
        sources.insert(test_source.source_id().clone(), test_source);

        let actor = IngestActor::new(
            Arc::new(sources),
            cmd_rx,
            msg_tx,
            plugin::types::ConnectionCtx::WebSocket(plugin::types::WebSocketConnectionCtx {
                auth: None,
                addr: "127.0.0.1:8000".parse().unwrap(),
            }),
            Some(SlowPlugin),
        );

        tokio::spawn(actor.run());

        cmd_tx
            .send(Command::Subscribe {
                sources: vec!["test".to_string()],
            })
            .unwrap();

        let msg = msg_rx.recv().await.unwrap();
        match msg {
            Message::CommandResponse(CommandResponse::SubscribeOk { sources })
                if sources == vec!["test"] =>
            {
                ()
            }
            _ => panic!("actor should respond with correct subscribe ok message"),
        }

        tokio::spawn(async move {
            // Simulate a source that sends 100 msgs/sec for 10 seconds
            for i in 0..1000 {
                tokio::time::sleep(Duration::from_millis(10)).await;

                tx2.send(TestMessage {
                    payload: Some(i.to_string().as_bytes().to_owned()),
                })
                .unwrap();
            }
        });

        let mut lag_notice_received = false;

        // Ensure a lag notice is emitted
        for _ in 0..20 {
            let msg = msg_rx.recv().await.unwrap();

            if let Message::Notice(Notice::Lag { source, count }) = msg {
                assert_eq!(source, "test");
                assert!(count > 0);
                lag_notice_received = true;
                break;
            }
        }

        assert!(
            lag_notice_received,
            "actor should emit a lag notice when it falls behind"
        );
    }
}
