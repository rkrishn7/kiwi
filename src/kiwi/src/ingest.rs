use std::collections::{btree_map, BTreeMap};
use std::fmt::Debug;
use std::sync::Arc;

use futures::stream::select_all::select_all;
use futures::StreamExt;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio_stream::wrappers::errors::BroadcastStreamRecvError;
use tokio_stream::wrappers::BroadcastStream;

use crate::event::MutableEvent;
use crate::hook::intercept::{self, Intercept};
use crate::protocol::{Command, CommandResponse, Message, Notice};
use crate::source::{Source, SourceId, SourceMessage};

/// This actor is responsible for the following tasks:
/// - Processing commands as they become available
/// - Reading events from subscribed sources, processing them, and
///   forwarding them along its active subscriptions
pub struct IngestActor<S, T, M, I> {
    cmd_rx: UnboundedReceiver<Command>,
    msg_tx: UnboundedSender<Message<M>>,
    sources: Arc<BTreeMap<SourceId, S>>,
    /// Subscriptions this actor currently maintains for its handle
    subscriptions: BTreeMap<SourceId, BroadcastStream<SourceMessage<T>>>,
    connection_ctx: intercept::types::ConnectionCtx,
    /// Custom context provided by the authentication hook
    auth_ctx: Option<intercept::types::AuthCtx>,
    pre_forward: Option<I>,
}

#[derive(Debug)]
/// Represents the current state of the actor's main processing loop, defining what action
/// it should next take. The states here are externally-driven, meaning external
/// events cause state transitions. As a result, there is no starting state which
/// may depart from the traditional concept of a state machine
enum IngestActorState<T> {
    ReceivedCommand(Command),
    ReceivedSourceEvent((SourceId, T)),
    Lagged((SourceId, u64)),
}

impl<S, T, M, P> IngestActor<S, T, M, P>
where
    M: Clone + Send + Sync + 'static,
    S: Source<Result = T>,
    T: Into<intercept::types::EventCtx> + Into<M> + MutableEvent + Debug + Clone + Send + 'static,
    P: Intercept + Clone + Send + 'static,
{
    pub fn new(
        sources: Arc<BTreeMap<String, S>>,
        cmd_rx: UnboundedReceiver<Command>,
        msg_tx: UnboundedSender<Message<M>>,
        connection_ctx: intercept::types::ConnectionCtx,
        auth_ctx: Option<intercept::types::AuthCtx>,
        pre_forward: Option<P>,
    ) -> Self {
        Self {
            cmd_rx,
            msg_tx,
            sources,
            connection_ctx,
            auth_ctx,
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
                            Ok(event) => IngestActorState::ReceivedSourceEvent((source_id.clone(), event)),
                            Err(BroadcastStreamRecvError::Lagged(count)) => IngestActorState::Lagged((source_id.clone(), count))
                        }
                    },
                }
            };

            match next_state {
                IngestActorState::ReceivedCommand(cmd) => {
                    let response = self.handle_command(cmd).await;

                    self.msg_tx.send(Message::CommandResponse(response))?;
                }
                IngestActorState::ReceivedSourceEvent((source_id, event)) => match event {
                    SourceMessage::Result(event) => {
                        if let Err(e) = self.forward_event(event).await {
                            tracing::error!("Error while forwarding source event: {:?}", e);
                        }
                    }
                    SourceMessage::MetadataChanged(message) => {
                        if let Some(_) = self.subscriptions.remove(&source_id) {
                            self.msg_tx
                                .send(Message::Notice(Notice::SubscriptionClosed {
                                    source: source_id,
                                    message: Some(message),
                                }))?;
                        }
                    }
                },
                IngestActorState::Lagged((source_id, count)) => {
                    tracing::warn!("Actor lagged behind by {} messages for source {}. Continuing to read from oldest available message", count, source_id);

                    // If we fail to send the message, it means the receiving half of the message channel
                    // was dropped, in which case we want to complete execution
                    if self
                        .msg_tx
                        .send(Message::Notice(Notice::Lag {
                            source: source_id,
                            count,
                        }))
                        .is_err()
                    {
                        break;
                    }
                }
            }
        }

        Ok(())
    }

    async fn handle_command(&mut self, command: Command) -> CommandResponse {
        match command {
            Command::Subscribe { source_id } => match self.subscriptions.entry(source_id.clone()) {
                btree_map::Entry::Occupied(_) => CommandResponse::SubscribeError {
                    source_id,
                    error: "Source already has an active subscription".to_string(),
                },
                btree_map::Entry::Vacant(entry) => {
                    let response = if let Some(source) = self.sources.get(&source_id) {
                        entry.insert(BroadcastStream::new(source.subscribe()));
                        CommandResponse::SubscribeOk { source_id }
                    } else {
                        CommandResponse::SubscribeError {
                            source_id,
                            error: "No source exists with the specified ID".to_string(),
                        }
                    };

                    response
                }
            },
            Command::Unsubscribe { source_id } => {
                match self.subscriptions.entry(source_id.clone()) {
                    btree_map::Entry::Occupied(entry) => {
                        entry.remove();
                        CommandResponse::UnsubscribeOk { source_id }
                    }
                    btree_map::Entry::Vacant(_) => CommandResponse::UnsubscribeError {
                        source_id,
                        error: "Source does not have an active subscription".to_string(),
                    },
                }
            }
        }
    }

    async fn forward_event(&mut self, event: T) -> anyhow::Result<()> {
        let plugin_event_ctx: intercept::types::EventCtx = event.clone().into();
        let plugin_ctx = intercept::types::Context {
            auth: self.auth_ctx.clone(),
            connection: self.connection_ctx.clone(),
            event: plugin_event_ctx,
        };

        let action = if let Some(plugin) = self.pre_forward.clone() {
            let result =
                tokio::task::spawn_blocking(move || plugin.intercept(&plugin_ctx)).await??;

            Some(result)
        } else {
            None
        };

        match action {
            Some(intercept::types::Action::Discard) => (),
            None | Some(intercept::types::Action::Forward) => {
                self.msg_tx.send(Message::Result(event.into()))?;
            }
            Some(intercept::types::Action::Transform(payload)) => {
                let transformed = event.set_payload(payload);

                self.msg_tx.send(Message::Result(transformed.into()))?;
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::source::SourceMessage;

    use super::*;
    use std::time::Duration;
    use tokio::sync::broadcast::{Receiver, Sender};

    #[derive(Debug, Clone)]
    struct TestSource {
        tx: Sender<SourceMessage<TestMessage>>,
        source_id: SourceId,
    }

    #[derive(Debug, Clone)]
    struct TestMessage {
        payload: Option<Vec<u8>>,
    }

    impl From<TestMessage> for intercept::types::EventCtx {
        fn from(value: TestMessage) -> Self {
            Self::Kafka(intercept::types::KafkaEventCtx {
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
        type Result = TestMessage;

        fn subscribe(&self) -> Receiver<SourceMessage<Self::Result>> {
            self.tx.subscribe()
        }

        fn source_id(&self) -> &SourceId {
            &self.source_id
        }
    }

    #[derive(Debug, Clone)]
    /// Discards all events
    struct DiscardPlugin;

    impl Intercept for DiscardPlugin {
        fn intercept(
            &self,
            _ctx: &intercept::types::Context,
        ) -> anyhow::Result<intercept::types::Action> {
            Ok(intercept::types::Action::Discard)
        }
    }

    fn send_subscribe_cmd(cmd_tx: &UnboundedSender<Command>, source_id: &str) {
        cmd_tx
            .send(Command::Subscribe {
                source_id: source_id.to_string(),
            })
            .unwrap();
    }

    fn send_unsubscribe_cmd(cmd_tx: &UnboundedSender<Command>, source_id: &str) {
        cmd_tx
            .send(Command::Unsubscribe {
                source_id: source_id.to_string(),
            })
            .unwrap();
    }

    fn spawn_actor<P: Intercept + Clone + Send + Sync + 'static>(
        pre_forward: Option<P>,
        test_source_ids: Vec<String>,
        source_channel_capacity: usize,
    ) -> (
        UnboundedSender<Command>,
        UnboundedReceiver<Message<TestMessage>>,
        Sender<SourceMessage<TestMessage>>,
        tokio::task::JoinHandle<anyhow::Result<()>>,
    ) {
        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel::<Command>();
        let (msg_tx, msg_rx) = tokio::sync::mpsc::unbounded_channel::<Message<TestMessage>>();

        let (source_tx, _) =
            tokio::sync::broadcast::channel::<SourceMessage<TestMessage>>(source_channel_capacity);

        let mut sources = BTreeMap::new();
        sources.extend(test_source_ids.into_iter().map(|source_id| {
            (
                source_id.clone(),
                TestSource {
                    tx: source_tx.clone(),
                    source_id,
                },
            )
        }));

        let actor = IngestActor::new(
            Arc::new(sources),
            cmd_rx,
            msg_tx,
            intercept::types::ConnectionCtx::WebSocket(intercept::types::WebSocketConnectionCtx {
                addr: "127.0.0.1:8000".parse().unwrap(),
            }),
            None,
            pre_forward,
        );

        let handle = tokio::spawn(actor.run());

        (cmd_tx, msg_rx, source_tx, handle)
    }

    async fn recv_subscribe_ok(
        rx: &mut UnboundedReceiver<Message<TestMessage>>,
        original_source_id: &str,
    ) {
        match rx.recv().await.unwrap() {
            Message::CommandResponse(CommandResponse::SubscribeOk { source_id }) => {
                assert_eq!(
                    source_id, original_source_id,
                    "source ID should match the one found in the initial subscribe command"
                );
            }
            m => panic!(
                "actor should respond with a subscribe ok message. Instead responded with {:?}",
                m
            ),
        }
    }

    async fn recv_subscribe_err(
        rx: &mut UnboundedReceiver<Message<TestMessage>>,
        original_source_id: &str,
    ) {
        match rx.recv().await.unwrap() {
            Message::CommandResponse(CommandResponse::SubscribeError { source_id, .. }) => {
                assert_eq!(
                    source_id, original_source_id,
                    "source ID should match the one found in the initial subscribe command"
                );
            }
            m => panic!(
                "actor should respond with an subscribe error message. Instead responded with {:?}",
                m
            ),
        }
    }

    async fn recv_unsubscribe_ok(
        rx: &mut UnboundedReceiver<Message<TestMessage>>,
        original_source_id: &str,
    ) {
        match rx.recv().await.unwrap() {
            Message::CommandResponse(CommandResponse::UnsubscribeOk { source_id }) => {
                assert_eq!(
                    source_id, original_source_id,
                    "source ID should match the one found in the initial unsubscribe command"
                );
            }
            m => panic!(
                "actor should respond with an unsubscribe ok message. Instead responded with {:?}",
                m
            ),
        }
    }

    async fn recv_unsubscribe_err(
        rx: &mut UnboundedReceiver<Message<TestMessage>>,
        original_source_id: &str,
    ) {
        match rx.recv().await.unwrap() {
            Message::CommandResponse(CommandResponse::UnsubscribeError { source_id, .. }) => {
                assert_eq!(
                    source_id, original_source_id,
                    "source ID should match the one found in the initial unsubscribe command"
                );
            }
            m => panic!("actor should respond with an unsubscribe error message. Instead responded with {:?}", m),
        }
    }

    #[tokio::test]
    async fn test_actor_completes_on_cmd_rx_drop() {
        let (cmd_tx, _, _, actor_handle) =
            spawn_actor(Some(DiscardPlugin), vec!["test".to_string()], 100);

        // Drop the command channel, which should cause the actor to complete
        drop(cmd_tx);

        assert!(
            actor_handle.await.unwrap().is_ok(),
            "ingest actor should complete successfully when command channel is closed"
        );
    }

    #[tokio::test]
    async fn test_source_subscribing() {
        let (cmd_tx, mut msg_rx, _, _) =
            spawn_actor(Some(DiscardPlugin), vec!["test".to_string()], 100);

        send_subscribe_cmd(&cmd_tx, "test");

        recv_subscribe_ok(&mut msg_rx, "test").await;

        // Ensure resubscribing to the same source results in an error
        send_subscribe_cmd(&cmd_tx, "test");

        recv_subscribe_err(&mut msg_rx, "test").await;

        // Subscribting to a non-existent source should result in an error
        send_subscribe_cmd(&cmd_tx, "test2");

        recv_subscribe_err(&mut msg_rx, "test2").await;
    }

    #[tokio::test]
    async fn test_source_unsubscribing() {
        let (cmd_tx, mut msg_rx, _, _) =
            spawn_actor(Some(DiscardPlugin), vec!["test".to_string()], 100);

        // Check that unsubscribing from a non-existent subscription results in an error
        send_unsubscribe_cmd(&cmd_tx, "test");

        recv_unsubscribe_err(&mut msg_rx, "test").await;

        send_subscribe_cmd(&cmd_tx, "test");

        recv_subscribe_ok(&mut msg_rx, "test").await;

        // Ensure that we can unsubscribe from an existing subscription
        send_unsubscribe_cmd(&cmd_tx, "test");

        recv_unsubscribe_ok(&mut msg_rx, "test").await;
    }

    #[tokio::test]
    async fn test_plugin_discard_action() {
        let (cmd_tx, mut msg_rx, source_tx, _) =
            spawn_actor(Some(DiscardPlugin), vec!["test".to_string()], 100);

        send_subscribe_cmd(&cmd_tx, "test");

        recv_subscribe_ok(&mut msg_rx, "test").await;

        for _ in 0..10 {
            source_tx
                .send(SourceMessage::Result(TestMessage { payload: None }))
                .unwrap();
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

        impl Intercept for ForwardPlugin {
            fn intercept(
                &self,
                _ctx: &intercept::types::Context,
            ) -> anyhow::Result<intercept::types::Action> {
                Ok(intercept::types::Action::Forward)
            }
        }

        let (cmd_tx, mut msg_rx, source_tx, _) =
            spawn_actor(Some(ForwardPlugin), vec!["test".to_string()], 100);

        send_subscribe_cmd(&cmd_tx, "test");

        recv_subscribe_ok(&mut msg_rx, "test").await;

        let num_messages = 10;

        for _ in 0..num_messages {
            source_tx
                .send(SourceMessage::Result(TestMessage { payload: None }))
                .unwrap();
        }

        let received_all_messages = {
            for _ in 0..num_messages {
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

        impl Intercept for TransformPlugin {
            fn intercept(
                &self,
                _ctx: &intercept::types::Context,
            ) -> anyhow::Result<intercept::types::Action> {
                Ok(intercept::types::Action::Transform(Some(
                    "hello".as_bytes().to_owned(),
                )))
            }
        }

        let (cmd_tx, mut msg_rx, source_tx, _) =
            spawn_actor(Some(TransformPlugin), vec!["test".to_string()], 100);

        send_subscribe_cmd(&cmd_tx, "test");

        recv_subscribe_ok(&mut msg_rx, "test").await;

        for _ in 0..10 {
            source_tx
                .send(SourceMessage::Result(TestMessage { payload: None }))
                .unwrap();
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
                    _ => panic!("actor should forward message when transform action is returned from plugin"),
                }
            }
            true
        };

        assert!(received_all_messages, "actor should forward all messages");
    }

    #[tokio::test]
    async fn test_lag_notices() {
        #[derive(Debug, Clone)]
        /// Simulates a slow executing plugin
        struct SlowPlugin;

        // The bottleneck for the actor is now this plugin. Since the actor calls it
        // synchronously, it can only process around 10 messages/sec
        impl Intercept for SlowPlugin {
            fn intercept(
                &self,
                _ctx: &intercept::types::Context,
            ) -> anyhow::Result<intercept::types::Action> {
                std::thread::sleep(Duration::from_millis(100));
                Ok(intercept::types::Action::Forward)
            }
        }

        let (cmd_tx, mut msg_rx, source_tx, _) =
            spawn_actor(Some(SlowPlugin), vec!["test".to_string()], 100);

        send_subscribe_cmd(&cmd_tx, "test");

        recv_subscribe_ok(&mut msg_rx, "test").await;

        tokio::spawn(async move {
            // Simulate a source that sends 100 msgs/sec for 10 seconds
            for i in 0..1000 {
                tokio::time::sleep(Duration::from_millis(10)).await;

                source_tx
                    .send(SourceMessage::Result(TestMessage {
                        payload: Some(i.to_string().as_bytes().to_owned()),
                    }))
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
