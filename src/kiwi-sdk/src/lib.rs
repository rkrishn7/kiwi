//! # Kiwi SDK
//! This crate provides utilities necessary for writing [Kiwi](https://github.com/rkrishn7/kiwi) plugins.
//!
//! # Examples
//!
//! ## Intercept
//! ```rust
//! //! A simple intercept hook that discards odd numbers from all counter sources
//! use kiwi_sdk::hook::intercept::{intercept, Action, Context, CounterEventCtx, EventCtx};
//!
//! /// You must use the `#[intercept]` macro to define an intercept hook.
//! #[intercept]
//! fn handle(ctx: Context) -> Action {
//!     match ctx.event {
//!         // We only care about counter sources in this example
//!         EventCtx::Counter(CounterEventCtx {
//!             source_id: _,
//!             count,
//!         }) => {
//!             if count % 2 == 0 {
//!                 // Returning `Action::Forward` instructs Kiwi to forward the event
//!                 // to the associated client.
//!                 return Action::Forward;
//!             } else {
//!                 // Returning `Action::Discard` instructs Kiwi to discard the event,
//!                 // preventing it from reaching the associated client.
//!                 return Action::Discard;
//!             }
//!         }
//!         _ => {}
//!     }
//!
//!     Action::Forward
//! }
//! ```
//!
//! ## Authenticate
//! ```rust
//! //! A simple authenticate hook that allows all incoming HTTP requests
//! use kiwi_sdk::hook::authenticate::{authenticate, Outcome};
//! use kiwi_sdk::http::Request;
//!
//! /// You must use the `#[authenticate]` macro to define an authenticate hook.
//! #[authenticate]
//! fn handle(req: Request<()>) -> Outcome {
//!     // Returning `Outcome::Authenticate` instructs Kiwi to allow the connection to be established.
//!     Outcome::Authenticate
//! }
//! ```

pub mod hook;

#[doc(hidden)]
pub mod wit {
    #![allow(missing_docs)]

    wit_bindgen::generate!({
        path: "./wit",
        world: "internal",
    });
}

/// Re-export for macro use.
#[doc(hidden)]
pub use wit_bindgen;

pub mod http;
