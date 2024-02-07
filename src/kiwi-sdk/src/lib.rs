pub mod hook {
    pub use kiwi_macro::authenticate;
    pub use kiwi_macro::intercept;
}

#[doc(hidden)]
pub mod wit {
    #![allow(missing_docs)]
    wit_bindgen::generate!({
        world: "internal",
        runtime_path: "wit_bindgen::rt",
        path: "../wit",
    });
}

/// Re-export for macro use.
#[doc(hidden)]
pub use wit_bindgen;

pub mod types;
