/// Declares an operation which updates the payload of a source-agnostic event
pub trait MutableEvent {
    fn set_payload(self, payload: Option<Vec<u8>>) -> Self;
}

pub type EventPayload = Option<Vec<u8>>;
