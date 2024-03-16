//! This crate exports macros that are  are used to generate the necessary code
//! to turn a source file into a `Guest` module, suitable for compilation and
//! execution within Kiwi's embedded WASM component runtime ([wasmtime](https://github.com/bytecodealliance/wasmtime)).
//!
//! ### NOTE
//! This crate is intended for use only via the Kiwi SDK and should not be used directly.

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
            mod bindings {
                #![allow(missing_docs)]
                ::kiwi_sdk::wit_bindgen::generate!({
                    path: #WIT_PATH,
                    world: "intercept-hook",
                    runtime_path: "::kiwi_sdk::wit_bindgen::rt",
                });
            }

            struct Kiwi;

            impl bindings::Guest for Kiwi {
                fn intercept(ctx: self::bindings::kiwi::kiwi::intercept_types::Context) -> self::bindings::kiwi::kiwi::intercept_types::Action {
                    super::#func_name(ctx.into()).into()
                }
            }

            impl From<self::bindings::kiwi::kiwi::intercept_types::Context> for ::kiwi_sdk::hook::intercept::Context {
                fn from(value: self::bindings::kiwi::kiwi::intercept_types::Context) -> Self {
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

            impl From<self::bindings::kiwi::kiwi::intercept_types::EventCtx> for ::kiwi_sdk::hook::intercept::EventCtx {
                fn from(value: self::bindings::kiwi::kiwi::intercept_types::EventCtx) -> Self {
                    match value {
                        self::bindings::kiwi::kiwi::intercept_types::EventCtx::Kafka(ctx) => Self::Kafka(ctx.into()),
                        self::bindings::kiwi::kiwi::intercept_types::EventCtx::Counter(ctx) => Self::Counter(ctx.into()),
                    }
                }
            }

            impl From<self::bindings::kiwi::kiwi::intercept_types::CounterEventCtx> for ::kiwi_sdk::hook::intercept::CounterEventCtx {
                fn from(value: self::bindings::kiwi::kiwi::intercept_types::CounterEventCtx) -> Self {
                    Self {
                        source_id: value.source_id,
                        count: value.count,
                    }
                }
            }

            impl From<self::bindings::kiwi::kiwi::intercept_types::KafkaEventCtx> for ::kiwi_sdk::hook::intercept::KafkaEventCtx {
                fn from(value: self::bindings::kiwi::kiwi::intercept_types::KafkaEventCtx) -> Self {
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

            impl From<self::bindings::kiwi::kiwi::intercept_types::ConnectionCtx> for ::kiwi_sdk::hook::intercept::ConnectionCtx {
                fn from(value: self::bindings::kiwi::kiwi::intercept_types::ConnectionCtx) -> Self {
                    match value {
                        self::bindings::kiwi::kiwi::intercept_types::ConnectionCtx::Websocket(ctx) => Self::WebSocket(ctx.into()),
                    }
                }
            }

            impl From<self::bindings::kiwi::kiwi::intercept_types::Websocket> for ::kiwi_sdk::hook::intercept::WebSocketConnectionCtx {
                fn from(value: self::bindings::kiwi::kiwi::intercept_types::Websocket) -> Self {
                    Self {
                        addr: value.addr,
                    }
                }
            }

            impl From<::kiwi_sdk::hook::intercept::Action> for self::bindings::kiwi::kiwi::intercept_types::Action {
                fn from(value: ::kiwi_sdk::hook::intercept::Action) -> Self {
                    match value {
                        ::kiwi_sdk::hook::intercept::Action::Forward => Self::Forward,
                        ::kiwi_sdk::hook::intercept::Action::Discard => Self::Discard,
                        ::kiwi_sdk::hook::intercept::Action::Transform(payload) => Self::Transform(payload.into()),
                    }
                }
            }

            impl From<::kiwi_sdk::hook::intercept::TransformedPayload> for self::bindings::kiwi::kiwi::intercept_types::TransformedPayload {
                fn from(value: ::kiwi_sdk::hook::intercept::TransformedPayload) -> Self {
                    match value {
                        ::kiwi_sdk::hook::intercept::TransformedPayload::Kafka(payload) => Self::Kafka(payload),
                        ::kiwi_sdk::hook::intercept::TransformedPayload::Counter(count) => Self::Counter(count),
                    }
                }
            }

            bindings::export!(Kiwi with_types_in bindings);
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
            mod bindings {
                #![allow(missing_docs)]
                ::kiwi_sdk::wit_bindgen::generate!({
                    path: #WIT_PATH,
                    world: "authenticate-hook",
                    runtime_path: "::kiwi_sdk::wit_bindgen::rt",
                    with: {
                        "wasi:http/outgoing-handler@0.2.0": ::kiwi_sdk::wit::wasi::http::outgoing_handler,
                        "wasi:http/types@0.2.0": ::kiwi_sdk::wit::wasi::http::types,
                        "wasi:clocks/monotonic-clock@0.2.0": ::kiwi_sdk::wit::wasi::clocks::monotonic_clock,
                        "wasi:io/poll@0.2.0": ::kiwi_sdk::wit::wasi::io::poll,
                        "wasi:io/streams@0.2.0": ::kiwi_sdk::wit::wasi::io::streams,
                        "wasi:io/error@0.2.0": ::kiwi_sdk::wit::wasi::io::error,
                    },
                });
            }

            struct Kiwi;

            impl bindings::Guest for Kiwi {
                fn authenticate(incoming: self::bindings::kiwi::kiwi::authenticate_types::HttpRequest) -> self::bindings::kiwi::kiwi::authenticate_types::Outcome {
                    super::#func_name(incoming.into()).into()
                }
            }

            impl From<::kiwi_sdk::hook::authenticate::Outcome> for self::bindings::kiwi::kiwi::authenticate_types::Outcome {
                fn from(value: ::kiwi_sdk::hook::authenticate::Outcome) -> Self {
                    match value {
                        ::kiwi_sdk::hook::authenticate::Outcome::Authenticate => Self::Authenticate,
                        ::kiwi_sdk::hook::authenticate::Outcome::Reject => Self::Reject,
                        ::kiwi_sdk::hook::authenticate::Outcome::WithContext(payload) => Self::WithContext(payload),
                    }
                }
            }

            impl From<self::bindings::kiwi::kiwi::authenticate_types::HttpRequest> for ::kiwi_sdk::http::Request<()> {
                fn from(value: self::bindings::kiwi::kiwi::authenticate_types::HttpRequest) -> Self {
                    let mut uri_builder = ::kiwi_sdk::http::Uri::builder();

                    if let Some(scheme) = value.scheme {
                        let scheme = match scheme {
                            ::kiwi_sdk::wit::wasi::http::types::Scheme::Http => ::kiwi_sdk::http::Scheme::HTTP,
                            ::kiwi_sdk::wit::wasi::http::types::Scheme::Https => ::kiwi_sdk::http::Scheme::HTTPS,
                            ::kiwi_sdk::wit::wasi::http::types::Scheme::Other(scheme) => scheme.as_str().parse().expect("failed to parse scheme"),
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

            bindings::export!(Kiwi with_types_in bindings);
        }
    )
        .into()
}
