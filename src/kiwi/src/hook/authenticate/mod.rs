pub mod types;
pub mod wasm;

use self::types::Outcome;
use tokio_tungstenite::tungstenite::http::Request as HttpRequest;

pub trait Authenticate {
    fn authenticate(&self, request: &HttpRequest<()>) -> anyhow::Result<Outcome>;
}
