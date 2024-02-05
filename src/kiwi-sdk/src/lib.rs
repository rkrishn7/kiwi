pub mod hook {
    pub use kiwi_macro::intercept;
}

/// Re-export for macro use.
#[doc(hidden)]
pub use wit_bindgen;

pub mod types;
