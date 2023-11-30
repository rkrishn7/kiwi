#[derive(Debug, Clone)]
pub enum Outcome {
    Authenticate,
    Reject,
    WithContext(Vec<u8>),
}
