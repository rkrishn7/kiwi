//! Bridge between WIT types and local plugin types
use super::bindgen::kiwi::kiwi::authenticate_types::*;
use crate::hook::authenticate::types;
use tokio_tungstenite::tungstenite::http::Request as HttpRequest;

impl From<Outcome> for types::Outcome {
    fn from(value: Outcome) -> Self {
        match value {
            Outcome::Authenticate => Self::Authenticate,
            Outcome::Reject => Self::Reject,
            Outcome::WithContext(payload) => Self::WithContext(payload),
        }
    }
}

impl From<HttpRequest<()>> for Request {
    fn from(value: HttpRequest<()>) -> Self {
        Self {
            uri: value.uri().to_string(),
            headers: value
                .headers()
                .iter()
                .map(|(k, v)| (k.as_str().to_string(), v.as_bytes().to_vec()))
                .collect(),
        }
    }
}
