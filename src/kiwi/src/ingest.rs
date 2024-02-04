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
use crate::source::{Source, SourceId, SourceMessage, SourceResult};
use crate::subscription::{Subscription, SubscriptionRecvError};

/// This actor is responsible for the following tasks:
/// - Processing commands as they become available
/// - Reading events from subscribed sources, processing them, and
///   forwarding them along its active subscriptions
/// TODO: Rename to SubscriptionManager
pub struct IngestActor<I> {
    /// Channel for receiving commands from the connection
    cmd_rx: UnboundedReceiver<Command>,
    /// Channel for sending messages to the connection
    msg_tx: UnboundedSender<Message>,
    /// Map of available sources
    sources: Arc<Mutex<BTreeMap<SourceId, Box<dyn Source + Send + Sync + 'static>>>>,
    /// Subscriptions this actor currently maintains for its handle
    subscriptions: BTreeMap<SourceId, Subscription>,
    /// Context for the connection that this actor is associated with
    connection_ctx: intercept::types::ConnectionCtx,
    /// Custom context provided by the authentication hook
    auth_ctx: Option<intercept::types::AuthCtx>,
    /// Plugin that is executed before forwarding events to the client
    intercept: Option<I>,
    /// Subscriber configuration that applies to all subscriptions managed
    /// by this actor
    subscriber_config: SubscriberConfig,
}

#[derive(Debug)]
/// Represents the current state of the actor's main processing loop, defining what action
/// it should next take. The states here are externally-driven, meaning external
/// events cause state transitions. As a result, there is no starting state which
/// may depart from the traditional concept of a state machine
enum IngestActorState<T> {
    /// A command has been received from the connection
    Command(Command),
    /// Results have been received from a source
    SourceResults((SourceId, Vec<T>)),
    /// An error has occurred while processing source results
    Error((SourceId, SubscriptionRecvError)),
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
                //
                // TODO(rkrishn7): This is likely expensive, especially as the number of subscriptions
                // increases. We should consider a more efficient way to combine source streams
                let mut combined = select_all(self.subscriptions.iter_mut().map(
                    |(source_id, subscription)| {
                        crate::util::stream::with_id(source_id, subscription.source_stream())
                    },
                ));

                tokio::select! {
                    biased;

                    maybe_cmd = self.cmd_rx.recv() => {
                        match maybe_cmd {
                            Some(cmd) => IngestActorState::Command(cmd),
                            // If the command rx hung up, it indicates the connection
                            // has been dropped so we can safely exit
                            None => break,
                        }
                    },
                    // Since the stream combinator is re-computed on each iteration, receiving
                    // `None` does not signal we are done. It is very possible that the actor
                    // handle later signals to add a new subscription via `cmd_tx`
                    Some((source_id, res)) = combined.next() => {
                        match res {
                            Ok(results) => IngestActorState::SourceResults((source_id.clone(), results)),
                            Err(err) => IngestActorState::Error((source_id.clone(), err)),
                        }
                    },
                }
            };

            match next_state {
                IngestActorState::Command(cmd) => {
                    self.handle_command(cmd).await?;
                }
                IngestActorState::SourceResults((source_id, results)) => {
                    for result in results {
                        let source_id = source_id.clone();
                        match result {
                            SourceMessage::Result(incoming) => {
                                self.forward_source_result(incoming).await?;
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
                IngestActorState::Error((source_id, err)) => match err {
                    SubscriptionRecvError::SubscriberLag(lag) => {
                        if let Some(threshold) = self.subscriber_config.lag_notice_threshold {
                            if lag >= threshold {
                                self.msg_tx.send(Message::Notice(Notice::Lag {
                                    source: source_id,
                                    count: lag,
                                }))?;
                            }
                        }
                    }
                    SubscriptionRecvError::ProcessLag(lag) => {
                        tracing::warn!(lag, source_id, connection = ?self.connection_ctx, "Receiver is lagging");
                        self.msg_tx.send(Message::Notice(Notice::Lag {
                            source: source_id,
                            count: lag,
                        }))?;
                    }
                    SubscriptionRecvError::SourceClosed => {
                        if self.subscriptions.remove(&source_id).is_some() {
                            self.msg_tx
                                .send(Message::Notice(Notice::SubscriptionClosed {
                                    source: source_id,
                                    message: Some("Source closed".to_string()),
                                }))?;
                        }
                    }
                },
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
            tokio::task::spawn_blocking(move || plugin.intercept(&plugin_ctx)).await??
        } else {
            intercept::types::Action::Forward
        };

        let processed: Option<SourceResult> = match action {
            intercept::types::Action::Discard => None,
            intercept::types::Action::Forward => Some(event),
            intercept::types::Action::Transform(payload) => {
                // Update event with new payload
                match event {
                    SourceResult::Kafka(ref mut kafka_event) => {
                        kafka_event.payload = payload;
                    }
                }

                Some(event)
            }
        };

        Ok(processed)
    }

    /// Forward the source result along the connection's message channel
    async fn forward_source_result(&mut self, incoming: SourceResult) -> anyhow::Result<()> {
        let incoming = self.process_source_result(incoming).await?;
        if let Some(incoming) = incoming {
            self.msg_tx.send(incoming.into())?;
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

    fn send_subscribe_cmd(
        cmd_tx: &UnboundedSender<Command>,
        source_id: &str,
        mode: Option<protocol::SubscriptionMode>,
    ) {
        cmd_tx
            .send(Command::Subscribe {
                source_id: source_id.to_string(),
                mode: mode.unwrap_or_default(),
            })
            .unwrap();
    }

    fn send_request_cmd(cmd_tx: &UnboundedSender<Command>, source_id: &str, n: u64) {
        cmd_tx
            .send(Command::Request {
                source_id: source_id.to_string(),
                n,
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
        subscriber_config: Option<SubscriberConfig>,
    ) -> (
        UnboundedSender<Command>,
        UnboundedReceiver<Message>,
        Sender<SourceMessage>,
        tokio::task::JoinHandle<anyhow::Result<()>>,
        Arc<Mutex<BTreeMap<String, Box<dyn Source + Send + Sync>>>>,
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

        let sources = Arc::new(Mutex::new(sources));

        let actor = IngestActor::new(
            Arc::clone(&sources),
            cmd_rx,
            msg_tx,
            intercept::types::ConnectionCtx::WebSocket(intercept::types::WebSocketConnectionCtx {
                addr: "127.0.0.1:8000".parse().unwrap(),
            }),
            None,
            pre_forward,
            subscriber_config.unwrap_or_default(),
        );

        let handle = tokio::spawn(actor.run());

        (cmd_tx, msg_rx, source_tx, handle, sources)
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

    async fn recv_request_ok(
        rx: &mut UnboundedReceiver<Message>,
        original_source_id: &str,
        expected_requests: Option<u64>,
    ) {
        match rx.recv().await.unwrap() {
            Message::CommandResponse(CommandResponse::RequestOk {
                source_id,
                requests,
            }) => {
                assert_eq!(source_id, original_source_id);
                if let Some(expected_requests) = expected_requests {
                    assert_eq!(requests, expected_requests);
                }
            }
            m => panic!(
                "actor should respond with a request ok message. Instead responded with {:?}",
                m
            ),
        }
    }

    async fn recv_request_err(rx: &mut UnboundedReceiver<Message>, original_source_id: &str) {
        match rx.recv().await.unwrap() {
            Message::CommandResponse(CommandResponse::RequestError { source_id, .. }) => {
                assert_eq!(source_id, original_source_id);
            }
            m => panic!(
                "actor should respond with a request error message. Instead responded with {:?}",
                m
            ),
        }
    }

    async fn recv_subscription_closed(
        rx: &mut UnboundedReceiver<Message>,
        original_source_id: &str,
    ) {
        match rx.recv().await.unwrap() {
            Message::Notice(Notice::SubscriptionClosed { source, .. }) => {
                assert_eq!(source, original_source_id);
            }
            m => panic!(
                "actor should respond with a subscription closed notice. Instead responded with {:?}",
                m
            ),
        }
    }

    async fn recv_lag_notice(rx: &mut UnboundedReceiver<Message>, source_id: &str, lag: u64) {
        match rx.recv().await.unwrap() {
            Message::Notice(Notice::Lag { source, count }) => {
                assert_eq!(source, source_id);
                assert_eq!(count, lag);
            }
            m => panic!(
                "actor should respond with a lag notice. Instead responded with {:?}",
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
        let (cmd_tx, _, _, actor_handle, _) =
            spawn_actor::<DiscardPlugin>(None, vec!["test".to_string()], 100, None);

        // Drop the command channel, which should cause the actor to complete
        drop(cmd_tx);

        assert!(
            actor_handle.await.unwrap().is_ok(),
            "ingest actor should complete successfully when command channel is closed"
        );
    }

    #[tokio::test]
    async fn test_source_subscribing() {
        let (cmd_tx, mut msg_rx, _, _, _) =
            spawn_actor(Some(DiscardPlugin), vec!["test".to_string()], 100, None);

        send_subscribe_cmd(&cmd_tx, "test", Some(protocol::SubscriptionMode::Push));

        recv_subscribe_ok(&mut msg_rx, "test").await;

        // Ensure resubscribing to the same source results in an error
        send_subscribe_cmd(&cmd_tx, "test", Some(protocol::SubscriptionMode::Push));

        recv_subscribe_err(&mut msg_rx, "test").await;

        // Subscribting to a non-existent source should result in an error
        send_subscribe_cmd(&cmd_tx, "test2", Some(protocol::SubscriptionMode::Push));

        recv_subscribe_err(&mut msg_rx, "test2").await;
    }

    #[tokio::test]
    async fn test_source_unsubscribing() {
        let (cmd_tx, mut msg_rx, _, _, _) =
            spawn_actor(Some(DiscardPlugin), vec!["test".to_string()], 100, None);

        // Check that unsubscribing from a non-existent subscription results in an error
        send_unsubscribe_cmd(&cmd_tx, "test");

        recv_unsubscribe_err(&mut msg_rx, "test").await;

        send_subscribe_cmd(&cmd_tx, "test", Some(protocol::SubscriptionMode::Push));

        recv_subscribe_ok(&mut msg_rx, "test").await;

        // Ensure that we can unsubscribe from an existing subscription
        send_unsubscribe_cmd(&cmd_tx, "test");

        recv_unsubscribe_ok(&mut msg_rx, "test").await;
    }

    #[tokio::test]
    async fn test_plugin_discard_action() {
        let (cmd_tx, mut msg_rx, source_tx, _, _) =
            spawn_actor(Some(DiscardPlugin), vec!["test".to_string()], 100, None);

        send_subscribe_cmd(&cmd_tx, "test", Some(protocol::SubscriptionMode::Push));

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

        let (cmd_tx, mut msg_rx, source_tx, _, _) =
            spawn_actor(Some(ForwardPlugin), vec!["test".to_string()], 100, None);

        send_subscribe_cmd(&cmd_tx, "test", Some(protocol::SubscriptionMode::Push));

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

        let (cmd_tx, mut msg_rx, source_tx, _, _) =
            spawn_actor(Some(TransformPlugin), vec!["test".to_string()], 100, None);

        send_subscribe_cmd(&cmd_tx, "test", Some(protocol::SubscriptionMode::Push));

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
    async fn test_source_closes_on_metadata_changed() {
        let (cmd_tx, mut msg_rx, source_tx, _, _) =
            spawn_actor::<DiscardPlugin>(None, vec!["test".to_string()], 100, None);

        send_subscribe_cmd(&cmd_tx, "test", Some(protocol::SubscriptionMode::Push));

        recv_subscribe_ok(&mut msg_rx, "test").await;

        source_tx
            .send(SourceMessage::MetadataChanged("Metadata changed".into()))
            .unwrap();

        recv_subscription_closed(&mut msg_rx, "test").await;
    }

    #[tokio::test]
    async fn test_source_closes_on_upstream_source_closed() {
        let (cmd_tx, mut msg_rx, source_tx, _, sources) =
            spawn_actor::<DiscardPlugin>(None, vec!["test".to_string()], 100, None);

        send_subscribe_cmd(&cmd_tx, "test", Some(protocol::SubscriptionMode::Push));

        recv_subscribe_ok(&mut msg_rx, "test").await;

        drop(source_tx);

        // Drop the source, which should cause the actor to emit a subscription closed notice
        let mut sources = sources.lock().unwrap();

        sources.remove("test");

        recv_subscription_closed(&mut msg_rx, "test").await;
    }

    #[tokio::test]
    async fn test_emits_lag_notice_on_subscriber_lag() {
        let (cmd_tx, mut msg_rx, source_tx, _, _) = spawn_actor::<DiscardPlugin>(
            None,
            vec!["test".to_string()],
            100,
            Some(SubscriberConfig {
                buffer_capacity: None,
                lag_notice_threshold: Some(2),
            }),
        );

        send_subscribe_cmd(&cmd_tx, "test", Some(protocol::SubscriptionMode::Pull));

        recv_subscribe_ok(&mut msg_rx, "test").await;

        for _ in 0..2 {
            source_tx
                .send(SourceMessage::Result(test_source_result()))
                .unwrap();
        }

        recv_lag_notice(&mut msg_rx, "test", 2).await;
    }

    #[tokio::test]
    async fn test_disallows_requests_cmds_for_push_subscriptions() {
        let (cmd_tx, mut msg_rx, _, _, _) =
            spawn_actor::<DiscardPlugin>(None, vec!["test".to_string()], 100, None);

        send_subscribe_cmd(&cmd_tx, "test", Some(protocol::SubscriptionMode::Push));

        recv_subscribe_ok(&mut msg_rx, "test").await;

        send_request_cmd(&cmd_tx, "test", 3);

        recv_request_err(&mut msg_rx, "test").await;
    }

    #[tokio::test]
    async fn test_handles_pull_subscription_requests() {
        let (cmd_tx, mut msg_rx, source_tx, _, _) =
            spawn_actor::<DiscardPlugin>(None, vec!["test".to_string()], 100, None);

        send_subscribe_cmd(&cmd_tx, "test", Some(protocol::SubscriptionMode::Pull));

        recv_subscribe_ok(&mut msg_rx, "test").await;

        send_request_cmd(&cmd_tx, "test", 3);

        recv_request_ok(&mut msg_rx, "test", Some(3)).await;

        send_request_cmd(&cmd_tx, "test", 10);

        recv_request_ok(&mut msg_rx, "test", Some(13)).await;

        for _ in 0..13 {
            source_tx
                .send(SourceMessage::Result(test_source_result()))
                .unwrap();
        }

        let received_all_messages = {
            for _ in 0..13 {
                let msg = msg_rx.recv().await.unwrap();
                match msg {
                    Message::Result(_) => (),
                    _ => panic!("actor should forward message when transform action is returned from plugin"),
                }
            }
            true
        };

        assert!(received_all_messages);
    }

    #[tokio::test]
    async fn test_emits_lag_notice_on_process_lag() {
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

        let (cmd_tx, mut msg_rx, source_tx, _, _) =
            spawn_actor(Some(SlowPlugin), vec!["test".to_string()], 100, None);

        send_subscribe_cmd(&cmd_tx, "test", Some(protocol::SubscriptionMode::Push));

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
