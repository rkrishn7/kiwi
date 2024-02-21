//! Types and macros for building authentication hooks

pub use kiwi_macro::authenticate;

/// The outcome of an authentication hook
pub enum Outcome {
    /// Authenticate the connection
    Authenticate,
    /// Reject the connection
    Reject,
    /// Authenticate the connection and attach custom context for use in the
    /// intercept hook
    WithContext(Vec<u8>),
}
