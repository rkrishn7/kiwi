pub mod types;
pub mod wasm;

use self::types::Outcome;
use tokio_tungstenite::tungstenite::http::Request as HttpRequest;

#[async_trait::async_trait]
pub trait Authenticate {
    async fn authenticate(&self, request: &HttpRequest<()>) -> anyhow::Result<Outcome>;
}
