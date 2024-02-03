use std::collections::{btree_map, BTreeMap};
use std::fmt::Debug;
use std::sync::{Arc, Mutex};

use futures::stream::select_all::select_all;
use futures::StreamExt;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio_stream::wrappers::BroadcastStream;

use crate::config::Subscriber as SubscriberConfig;
use crate::hook::intercept::{self, Intercept};
use crate::protocol::{Command, CommandResponse, Message, Notice};
use crate::source::{self, Source, SourceId, SourceMessage, SourceResult};
use crate::subscription::{Subscription, SubscriptionRecvError};

/// This actor is responsible for the following tasks:
/// - Processing commands as they become available
/// - Reading events from subscribed sources, processing them, and
///   forwarding them along its active subscriptions
/// TODO: Rename to SubscriptionManager
pub struct IngestActor<I> {
    cmd_rx: UnboundedReceiver<Command>,
    msg_tx: UnboundedSender<Message>,
    sources: Arc<Mutex<BTreeMap<SourceId, Box<dyn Source + Send + Sync + 'static>>>>,
    /// Subscriptions this actor currently maintains for its handle
    subscriptions: BTreeMap<SourceId, Subscription>,
    connection_ctx: intercept::types::ConnectionCtx,
    /// Custom context provided by the authentication hook
    auth_ctx: Option<intercept::types::AuthCtx>,
    /// Plugin that is executed before forwarding events to the client
    intercept: Option<I>,
    subscriber_config: SubscriberConfig,
}

#[derive(Debug)]
/// Represents the current state of the actor's main processing loop, defining what action
/// it should next take. The states here are externally-driven, meaning external
/// events cause state transitions. As a result, there is no starting state which
/// may depart from the traditional concept of a state machine
enum IngestActorState<T> {
    ReceivedCommand(Command),
    ReceivedSourceResults((SourceId, Vec<T>)),
    Lagged((SourceId, u64)),
}

impl<I> IngestActor<I>
where
    I: Intercept + Clone + Send + 'static,
{
    pub fn new(
        sources: Arc<Mutex<BTreeMap<SourceId, Box<dyn Source + Send + Sync + 'static>>>>,
        cmd_rx: UnboundedReceiver<Command>,
        msg_tx: UnboundedSender<Message>,
        connection_ctx: intercept::types::ConnectionCtx,
        auth_ctx: Option<intercept::types::AuthCtx>,
        intercept: Option<I>,
        subscriber_config: SubscriberConfig,
    ) -> Self {
        Self {
            cmd_rx,
            msg_tx,
            sources,
            connection_ctx,
            auth_ctx,
            subscriptions: Default::default(),
            intercept,
            subscriber_config,
        }
    }

    /// Drives this connection to completion by consuming from the specified stream
    pub async fn run(mut self) -> anyhow::Result<()> {
        loop {
            let next_state = {
                // Combine all the current subscriptions into a single stream
                let start = std::time::Instant::now();
                let mut combined = select_all(self.subscriptions.iter_mut().map(
                    |(source_id, subscription)| {
                        crate::util::stream::with_id(source_id, subscription.source_stream())
                    },
                ));
                let duration = start.elapsed();

                println!("Time constructing streams is: {:?}", duration);

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
                            Ok(results) => IngestActorState::ReceivedSourceResults((source_id.clone(), results)),
                            Err(SubscriptionRecvError::ProcessLag(count)) => IngestActorState::Lagged((source_id.clone(), count)),
                            Err(SubscriptionRecvError::SubscriberLag(count)) => IngestActorState::Lagged((source_id.clone(), count)),
                        }
                    },
                }
            };

            match next_state {
                IngestActorState::ReceivedCommand(cmd) => {
                    self.handle_command(cmd).await?;
                }
                IngestActorState::ReceivedSourceResults((source_id, results)) => {
                    for result in results {
                        let source_id = source_id.clone();
                        match result {
                            SourceMessage::Result(incoming) => {
                                if let Err(error) = self
                                    .forward_source_result(source_id.clone(), incoming)
                                    .await
                                {
                                    tracing::error!(
                                        source_id = source_id,
                                        error = ?error,
                                        "Failed to handle incoming result from source"
                                    );
                                }
                            }
                            SourceMessage::MetadataChanged(message) => {
                                if self.subscriptions.remove(&source_id).is_some() {
                                    self.msg_tx.send(Message::Notice(
                                        Notice::SubscriptionClosed {
                                            source: source_id,
                                            message: Some(message),
                                        },
                                    ))?;
                                }
                            }
                        }
                    }
                }
                IngestActorState::Lagged((source_id, count)) => {
                    tracing::warn!("Actor lagged behind by {} messages for source {}. Continuing to read from oldest available message", count, source_id);
                    self.msg_tx.send(Message::Notice(Notice::Lag {
                        source: source_id,
                        count,
                    }))?;
                }
            }
        }

        tracing::debug!(connection = ?self.connection_ctx, "Ingest actor completed normally");

        Ok(())
    }

    async fn handle_command(&mut self, command: Command) -> anyhow::Result<()> {
        match command {
            Command::Subscribe { source_id, mode } => {
                let response = match self.subscriptions.entry(source_id.clone()) {
                    btree_map::Entry::Occupied(_) => CommandResponse::SubscribeError {
                        source_id,
                        error: "Source already has an active subscription".to_string(),
                    },
                    btree_map::Entry::Vacant(entry) => {
                        let response = if let Some(source) =
                            self.sources.lock().expect("poisoned lock").get(&source_id)
                        {
                            let source_stream = BroadcastStream::new(source.subscribe());
                            let subscription = Subscription::from_mode(
                                source_stream,
                                mode,
                                self.subscriber_config.buffer_capacity,
                                self.subscriber_config.lag_notice_threshold,
                            );

                            entry.insert(subscription);

                            CommandResponse::SubscribeOk { source_id }
                        } else {
                            CommandResponse::SubscribeError {
                                source_id,
                                error: "No source exists with the specified ID".to_string(),
                            }
                        };

                        response
                    }
                };

                self.msg_tx.send(Message::CommandResponse(response))?;
            }
            Command::Unsubscribe { source_id } => {
                let response = match self.subscriptions.entry(source_id.clone()) {
                    btree_map::Entry::Occupied(entry) => {
                        entry.remove();
                        CommandResponse::UnsubscribeOk { source_id }
                    }
                    btree_map::Entry::Vacant(_) => CommandResponse::UnsubscribeError {
                        source_id,
                        error: "Source does not have an active subscription".to_string(),
                    },
                };

                self.msg_tx.send(Message::CommandResponse(response))?;
            }
            Command::Request { source_id, n } => {
                match self.subscriptions.entry(source_id.clone()) {
                    btree_map::Entry::Occupied(mut entry) => {
                        let subscription = entry.get_mut();
                        match subscription {
                            Subscription::Pull(subscription) => {
                                subscription.add_requests(n);
                                self.msg_tx.send(Message::CommandResponse(
                                    CommandResponse::RequestOk {
                                        source_id,
                                        requests: subscription.requests(),
                                    },
                                ))?;
                                // subscription.react_incoming(&self.msg_tx, None)?;
                            }
                            Subscription::Push(_) => {
                                self.msg_tx.send(Message::CommandResponse(
                                    CommandResponse::RequestError {
                                        source_id,
                                        error: "Source is not in pull mode".to_string(),
                                    },
                                ))?;
                            }
                        }
                    }
                    btree_map::Entry::Vacant(_) => {
                        self.msg_tx.send(Message::CommandResponse(
                            CommandResponse::UnsubscribeError {
                                source_id,
                                error: "Source does not have an active subscription".to_string(),
                            },
                        ))?;
                    }
                }
            }
        }

        Ok(())
    }

    /// Processes a source result by passing it through the intercept hook
    async fn process_source_result(
        &self,
        mut event: SourceResult,
    ) -> anyhow::Result<Option<SourceResult>> {
        let plugin_event_ctx: intercept::types::EventCtx = event.clone().into();
        let plugin_ctx = intercept::types::Context {
            auth: self.auth_ctx.clone(),
            connection: self.connection_ctx.clone(),
            event: plugin_event_ctx,
        };

        let action = if let Some(plugin) = self.intercept.clone() {
            let result =
                tokio::task::spawn_blocking(move || plugin.intercept(&plugin_ctx)).await??;

            Some(result)
        } else {
            None
        };

        let processed: Option<SourceResult> = match action {
            Some(intercept::types::Action::Discard) => None,
            None | Some(intercept::types::Action::Forward) => Some(event.into()),
            Some(intercept::types::Action::Transform(payload)) => {
                // Update event with new payload
                match event {
                    SourceResult::Kafka(ref mut kafka_event) => {
                        kafka_event.payload = payload;
                    }
                }

                Some(event.into())
            }
        };

        Ok(processed)
    }

    /// Forward the source result along the connection's message channel
    async fn forward_source_result(
        &mut self,
        source_id: SourceId,
        incoming: SourceResult,
    ) -> anyhow::Result<()> {
        let incoming = self.process_source_result(incoming).await?;

        let subscription = self.subscriptions.get_mut(&source_id);

        if let Some(subscription) = subscription {
            match subscription {
                Subscription::Pull(_) => {
                    // pull_state.react_incoming(&self.msg_tx, incoming)?;

                    // if pull_state.should_emit_lag_notice() {
                    //     self.msg_tx.send(Message::Notice(Notice::Lag {
                    //         source: source_id,
                    //         count: pull_state.lag,
                    //     }))?;
                    // }

                    if let Some(incoming) = incoming {
                        self.msg_tx.send(incoming.into())?;
                    }
                }
                Subscription::Push(_) => {
                    if let Some(incoming) = incoming {
                        self.msg_tx.send(incoming.into())?;
                    }
                }
            }
        } else {
            tracing::warn!(
                source_id = source_id,
                "Received event for source with no active subscription"
            );
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::protocol;
    use crate::source::{SourceMessage, SourceMetadata};

    use super::*;
    use std::time::Duration;
    use tokio::sync::broadcast::{Receiver, Sender};

    #[derive(Debug, Clone)]
    struct TestSource {
        tx: Sender<SourceMessage>,
        source_id: SourceId,
    }

    impl Source for TestSource {
        fn subscribe(&self) -> Receiver<SourceMessage> {
            self.tx.subscribe()
        }

        fn source_id(&self) -> &SourceId {
            &self.source_id
        }

        fn metadata_tx(&self) -> &Option<tokio::sync::mpsc::UnboundedSender<SourceMetadata>> {
            &None
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

    fn test_source_result() -> SourceResult {
        SourceResult::Kafka(crate::source::kafka::KafkaSourceResult {
            key: None,
            payload: None,
            topic: "test".to_string(),
            timestamp: None,
            partition: 0,
            offset: 0,
        })
    }

    fn send_subscribe_cmd(cmd_tx: &UnboundedSender<Command>, source_id: &str) {
        cmd_tx
            .send(Command::Subscribe {
                source_id: source_id.to_string(),
                mode: protocol::SubscriptionMode::Push,
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
        UnboundedReceiver<Message>,
        Sender<SourceMessage>,
        tokio::task::JoinHandle<anyhow::Result<()>>,
    ) {
        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel::<Command>();
        let (msg_tx, msg_rx) = tokio::sync::mpsc::unbounded_channel::<Message>();

        let (source_tx, _) =
            tokio::sync::broadcast::channel::<SourceMessage>(source_channel_capacity);

        let mut sources = BTreeMap::new();
        sources.extend(test_source_ids.into_iter().map(|source_id| {
            (
                source_id.clone(),
                Box::new(TestSource {
                    tx: source_tx.clone(),
                    source_id,
                }) as Box<dyn Source + Send + Sync + 'static>,
            )
        }));

        let actor = IngestActor::new(
            Arc::new(Mutex::new(sources)),
            cmd_rx,
            msg_tx,
            intercept::types::ConnectionCtx::WebSocket(intercept::types::WebSocketConnectionCtx {
                addr: "127.0.0.1:8000".parse().unwrap(),
            }),
            None,
            pre_forward,
            Default::default(),
        );

        let handle = tokio::spawn(actor.run());

        (cmd_tx, msg_rx, source_tx, handle)
    }

    async fn recv_subscribe_ok(rx: &mut UnboundedReceiver<Message>, original_source_id: &str) {
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

    async fn recv_subscribe_err(rx: &mut UnboundedReceiver<Message>, original_source_id: &str) {
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

    async fn recv_unsubscribe_ok(rx: &mut UnboundedReceiver<Message>, original_source_id: &str) {
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

    async fn recv_unsubscribe_err(rx: &mut UnboundedReceiver<Message>, original_source_id: &str) {
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
            spawn_actor::<DiscardPlugin>(None, vec!["test".to_string()], 100);

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
                .send(SourceMessage::Result(test_source_result()))
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
                .send(SourceMessage::Result(test_source_result()))
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
                .send(SourceMessage::Result(test_source_result()))
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
            for _ in 0..1000 {
                tokio::time::sleep(Duration::from_millis(10)).await;

                source_tx
                    .send(SourceMessage::Result(test_source_result()))
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
