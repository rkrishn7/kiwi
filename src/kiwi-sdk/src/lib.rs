pub mod hook {
    pub use kiwi_macro::authenticate;
    pub use kiwi_macro::intercept;
}

pub mod wasi {
    pub use kiwi_macro::use_wasi_http_types;

    pub mod http {
        pub use kiwi_macro::make_http_request_fn;
    }
}

/// Re-export for macro use.
#[doc(hidden)]
pub use wit_bindgen;

pub mod types;
