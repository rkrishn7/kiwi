use async_trait::async_trait;
use http::Request as HttpRequest;

#[derive(Debug, Clone)]
pub enum Outcome {
    Authenticate,
    Reject,
    WithContext(Vec<u8>),
}

#[async_trait]
pub trait Authenticate {
    async fn authenticate(&self, request: HttpRequest<()>) -> anyhow::Result<Outcome>;
}
