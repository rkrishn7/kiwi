pub enum Outcome {
    Authenticate,
    Reject,
    WithContext(Vec<u8>),
}
