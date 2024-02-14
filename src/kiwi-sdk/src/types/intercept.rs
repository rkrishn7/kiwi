#[derive(Debug, Clone)]
pub enum TransformedPayload {
    Kafka(Option<Vec<u8>>),
    Counter(u64),
}

#[derive(Debug, Clone)]
pub enum Action {
    Forward,
    Discard,
    Transform(TransformedPayload),
}

#[derive(Debug, Clone)]
pub struct Context {
    pub auth: Option<AuthCtx>,
    pub connection: ConnectionCtx,
    pub event: EventCtx,
}

#[derive(Debug, Clone)]
pub struct AuthCtx {
    pub raw: Vec<u8>,
}

#[derive(Debug, Clone)]
pub enum ConnectionCtx {
    WebSocket(WebSocketConnectionCtx),
}

#[derive(Debug, Clone)]
pub struct WebSocketConnectionCtx {
    pub addr: Option<String>,
}

#[derive(Debug, Clone)]
pub enum EventCtx {
    Kafka(KafkaEventCtx),
    Counter(CounterEventCtx),
}

#[derive(Debug, Clone)]
pub struct KafkaEventCtx {
    pub payload: Option<Vec<u8>>,
    pub topic: String,
    pub timestamp: Option<i64>,
    pub partition: i32,
    pub offset: i64,
}

#[derive(Debug, Clone)]
pub struct CounterEventCtx {
    pub source_id: String,
    pub count: u64,
}
