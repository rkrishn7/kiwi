//! This crate provides a set of macros for use via the Kiwi SDK. These macros
//! are used to generate the necessary code to turn a source file into a `Guest` module,
//! suitable for compilation and execution within Kiwi's embedded WASM component runtime
//! ([wasmtime](https://github.com/bytecodealliance/wasmtime)).
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

use proc_macro::TokenStream;
use quote::quote;

/// Wit sources are packaged as part of the release process, so we can reference them
/// from the crate root. We need to hoist the path here to ensure it references the correct
/// location.
const WIT_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/wit");

/// Macro necessary for creating an intercept hook.
#[proc_macro_attribute]
pub fn intercept(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let func = syn::parse_macro_input!(item as syn::ItemFn);
    let func_name = &func.sig.ident;

    // Note that intercept modules don't link in WASI, so there's no need
    // to remap any WASI imports to their counterparts located in the Kiwi SDK.
    // In fact, because Kiwi does not link in WASI for intercept modules, attempting
    // to use WASI imports should result in an error.
    quote!(
        #func
        mod __kiwi_intercept {
            mod preamble {
                #![allow(missing_docs)]
                ::kiwi_sdk::wit_bindgen::generate!({
                    path: #WIT_PATH,
                    world: "intercept-hook",
                    runtime_path: "::kiwi_sdk::wit_bindgen::rt",
                    exports: {
                        world: Kiwi,
                    }
                });

                pub struct Kiwi;
            }

            impl preamble::Guest for preamble::Kiwi {
                fn intercept(ctx: self::preamble::kiwi::kiwi::intercept_types::Context) -> self::preamble::kiwi::kiwi::intercept_types::Action {
                    super::#func_name(ctx.into()).into()
                }
            }

            impl From<self::preamble::kiwi::kiwi::intercept_types::Context> for ::kiwi_sdk::hook::intercept::Context {
                fn from(value: self::preamble::kiwi::kiwi::intercept_types::Context) -> Self {
                    Self {
                        auth: value.auth.map(|raw| {
                            ::kiwi_sdk::hook::intercept::AuthCtx {
                                raw,
                            }
                        }),
                        connection: value.connection.into(),
                        event: value.event.into(),
                    }
                }
            }

            impl From<self::preamble::kiwi::kiwi::intercept_types::EventCtx> for ::kiwi_sdk::hook::intercept::EventCtx {
                fn from(value: self::preamble::kiwi::kiwi::intercept_types::EventCtx) -> Self {
                    match value {
                        self::preamble::kiwi::kiwi::intercept_types::EventCtx::Kafka(ctx) => Self::Kafka(ctx.into()),
                        self::preamble::kiwi::kiwi::intercept_types::EventCtx::Counter(ctx) => Self::Counter(ctx.into()),
                    }
                }
            }

            impl From<self::preamble::kiwi::kiwi::intercept_types::CounterEventCtx> for ::kiwi_sdk::hook::intercept::CounterEventCtx {
                fn from(value: self::preamble::kiwi::kiwi::intercept_types::CounterEventCtx) -> Self {
                    Self {
                        source_id: value.source_id,
                        count: value.count,
                    }
                }
            }

            impl From<self::preamble::kiwi::kiwi::intercept_types::KafkaEventCtx> for ::kiwi_sdk::hook::intercept::KafkaEventCtx {
                fn from(value: self::preamble::kiwi::kiwi::intercept_types::KafkaEventCtx) -> Self {
                    let timestamp: Option<i64> = value.timestamp.map(|t| t.try_into().expect("timestamp conversion must not fail"));
                    let partition: i32 = value.partition.try_into().expect("partition conversion must not fail");
                    let offset: i64 = value.offset.try_into().expect("offset conversion must not fail");

                    Self {
                        payload: value.payload,
                        topic: value.topic,
                        timestamp,
                        partition,
                        offset,
                    }
                }
            }

            impl From<self::preamble::kiwi::kiwi::intercept_types::ConnectionCtx> for ::kiwi_sdk::hook::intercept::ConnectionCtx {
                fn from(value: self::preamble::kiwi::kiwi::intercept_types::ConnectionCtx) -> Self {
                    match value {
                        self::preamble::kiwi::kiwi::intercept_types::ConnectionCtx::Websocket(ctx) => Self::WebSocket(ctx.into()),
                    }
                }
            }

            impl From<self::preamble::kiwi::kiwi::intercept_types::Websocket> for ::kiwi_sdk::hook::intercept::WebSocketConnectionCtx {
                fn from(value: self::preamble::kiwi::kiwi::intercept_types::Websocket) -> Self {
                    Self {
                        addr: value.addr,
                    }
                }
            }

            impl From<::kiwi_sdk::hook::intercept::Action> for self::preamble::kiwi::kiwi::intercept_types::Action {
                fn from(value: ::kiwi_sdk::hook::intercept::Action) -> Self {
                    match value {
                        ::kiwi_sdk::hook::intercept::Action::Forward => Self::Forward,
                        ::kiwi_sdk::hook::intercept::Action::Discard => Self::Discard,
                        ::kiwi_sdk::hook::intercept::Action::Transform(payload) => Self::Transform(payload.into()),
                    }
                }
            }

            impl From<::kiwi_sdk::hook::intercept::TransformedPayload> for self::preamble::kiwi::kiwi::intercept_types::TransformedPayload {
                fn from(value: ::kiwi_sdk::hook::intercept::TransformedPayload) -> Self {
                    match value {
                        ::kiwi_sdk::hook::intercept::TransformedPayload::Kafka(payload) => Self::Kafka(payload),
                        ::kiwi_sdk::hook::intercept::TransformedPayload::Counter(count) => Self::Counter(count),
                    }
                }
            }
        }
    )
        .into()
}

/// Macro necessary for creating an authenticate hook.
#[proc_macro_attribute]
pub fn authenticate(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let func = syn::parse_macro_input!(item as syn::ItemFn);
    let func_name = &func.sig.ident;

    // Kiwi does link in WASI for authenticate modules, so we need to remap the
    // WASI imports to their counterparts located in the Kiwi SDK.
    quote!(
        #func
        mod __kiwi_authenticate {
            mod preamble {
                #![allow(missing_docs)]
                ::kiwi_sdk::wit_bindgen::generate!({
                    path: #WIT_PATH,
                    world: "authenticate-hook",
                    runtime_path: "::kiwi_sdk::wit_bindgen::rt",
                    exports: {
                        world: Kiwi,
                    },
                    with: {
                        "wasi:http/outgoing-handler@0.2.0": ::kiwi_sdk::wit::wasi::http::outgoing_handler,
                        "wasi:http/types@0.2.0": ::kiwi_sdk::wit::wasi::http::types,
                        "wasi:clocks/monotonic-clock@0.2.0": ::kiwi_sdk::wit::wasi::clocks::monotonic_clock,
                        "wasi:io/poll@0.2.0": ::kiwi_sdk::wit::wasi::io::poll,
                        "wasi:io/streams@0.2.0": ::kiwi_sdk::wit::wasi::io::streams,
                        "wasi:io/error@0.2.0": ::kiwi_sdk::wit::wasi::io::error,
                    },
                });

                pub struct Kiwi;
            }

            impl preamble::Guest for preamble::Kiwi {
                fn authenticate(incoming: self::preamble::kiwi::kiwi::authenticate_types::HttpRequest) -> self::preamble::kiwi::kiwi::authenticate_types::Outcome {
                    super::#func_name(incoming.into()).into()
                }
            }

            impl From<::kiwi_sdk::hook::authenticate::Outcome> for self::preamble::kiwi::kiwi::authenticate_types::Outcome {
                fn from(value: ::kiwi_sdk::hook::authenticate::Outcome) -> Self {
                    match value {
                        ::kiwi_sdk::hook::authenticate::Outcome::Authenticate => Self::Authenticate,
                        ::kiwi_sdk::hook::authenticate::Outcome::Reject => Self::Reject,
                        ::kiwi_sdk::hook::authenticate::Outcome::WithContext(payload) => Self::WithContext(payload),
                    }
                }
            }

            impl From<self::preamble::kiwi::kiwi::authenticate_types::HttpRequest> for ::kiwi_sdk::http::Request<()> {
                fn from(value: self::preamble::kiwi::kiwi::authenticate_types::HttpRequest) -> Self {
                    let mut uri_builder = ::kiwi_sdk::http::Uri::builder();

                    if let Some(scheme) = value.scheme {
                        let scheme = match scheme {
                            ::kiwi_sdk::wit::wasi::http::types::Scheme::Http => ::kiwi_sdk::http::Scheme::HTTP,
                            ::kiwi_sdk::wit::wasi::http::types::Scheme::Https => ::kiwi_sdk::http::Scheme::HTTPS,
                            ::kiwi_sdk::wit::wasi::http::types::Scheme::Other(scheme) => panic!("Unsupported scheme"),
                        };

                        uri_builder = uri_builder.scheme(scheme);
                    }

                    if let Some(authority) = value.authority {
                        uri_builder = uri_builder.authority(authority.as_str());
                    }

                    if let Some(path_with_query) = value.path_with_query {
                        uri_builder = uri_builder.path_and_query(path_with_query.as_str());
                    }

                    let uri = uri_builder.build().expect("failed to build uri");

                    let method = match value.method {
                        ::kiwi_sdk::wit::wasi::http::types::Method::Get => ::kiwi_sdk::http::Method::GET,
                        ::kiwi_sdk::wit::wasi::http::types::Method::Head => ::kiwi_sdk::http::Method::HEAD,
                        ::kiwi_sdk::wit::wasi::http::types::Method::Post => ::kiwi_sdk::http::Method::POST,
                        ::kiwi_sdk::wit::wasi::http::types::Method::Put => ::kiwi_sdk::http::Method::PUT,
                        ::kiwi_sdk::wit::wasi::http::types::Method::Delete => ::kiwi_sdk::http::Method::DELETE,
                        ::kiwi_sdk::wit::wasi::http::types::Method::Connect => ::kiwi_sdk::http::Method::CONNECT,
                        ::kiwi_sdk::wit::wasi::http::types::Method::Options => ::kiwi_sdk::http::Method::OPTIONS,
                        ::kiwi_sdk::wit::wasi::http::types::Method::Trace => ::kiwi_sdk::http::Method::TRACE,
                        ::kiwi_sdk::wit::wasi::http::types::Method::Patch => ::kiwi_sdk::http::Method::PATCH,
                        ::kiwi_sdk::wit::wasi::http::types::Method::Other(_) => panic!("Unknown method"),
                    };

                    let mut request_builder = ::kiwi_sdk::http::Request::builder()
                        .method(method)
                        .uri(uri);

                    for (key, value) in value.headers {
                        request_builder = request_builder.header(key, value);
                    }

                    request_builder.body(()).expect("failed to build request")
                }
            }
        }
    )
        .into()
}
