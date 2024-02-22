//! # Kiwi SDK
//! This crate provides a Rust interface to the [Kiwi](https://github.com/rkrishn7/kiwi) platform.

pub mod hook;

#[doc(hidden)]
pub mod wit {
    #![allow(missing_docs)]

    wit_bindgen::generate!({
        path: "../wit",
        world: "internal",
    });
}

/// Re-export for macro use.
#[doc(hidden)]
pub use wit_bindgen;

pub mod http;
