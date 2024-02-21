//! Types and macros for building intercept hooks

pub use kiwi_macro::intercept;

#[derive(Debug, Clone)]
/// Represents a transformed source payload
pub enum TransformedPayload {
    /// A transformed Kafka payload
    Kafka(Option<Vec<u8>>),
    /// A transformed counter payload
    Counter(u64),
}

#[derive(Debug, Clone)]
/// Represents an action to take for a given event
pub enum Action {
    /// Forward the event to the client
    Forward,
    /// Discard the event, preventing it from reaching the client
    Discard,
    /// Transform the event and send it to the client
    Transform(TransformedPayload),
}

#[derive(Debug, Clone)]
/// An event bundled with additional context
pub struct Context {
    /// The authentication context, if any
    pub auth: Option<AuthCtx>,
    /// The connection context
    pub connection: ConnectionCtx,
    /// The event context
    pub event: EventCtx,
}

#[derive(Debug, Clone)]
/// Represents the authentication context as provided by the authentication hook
pub struct AuthCtx {
    pub raw: Vec<u8>,
}

#[derive(Debug, Clone)]
/// Represents the connection context
pub enum ConnectionCtx {
    /// A WebSocket connection context
    WebSocket(WebSocketConnectionCtx),
}

#[derive(Debug, Clone)]
/// Represents the WebSocket connection context
pub struct WebSocketConnectionCtx {
    /// The IP address of the client
    pub addr: Option<String>,
}

#[derive(Debug, Clone)]
/// Represents the event context
pub enum EventCtx {
    /// A Kafka event context
    Kafka(KafkaEventCtx),
    /// A counter event context
    Counter(CounterEventCtx),
}

#[derive(Debug, Clone)]
/// A Kafka event context
pub struct KafkaEventCtx {
    /// The payload of the event
    pub payload: Option<Vec<u8>>,
    /// The topic to which the event was published
    pub topic: String,
    /// The timestamp of the event
    pub timestamp: Option<i64>,
    /// The topic partition of the event
    pub partition: i32,
    /// The offset of the event
    pub offset: i64,
}

#[derive(Debug, Clone)]
/// A counter event context
pub struct CounterEventCtx {
    /// The source ID of the counter source
    pub source_id: String,
    /// The current count of the counter
    pub count: u64,
}
