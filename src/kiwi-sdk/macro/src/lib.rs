use proc_macro::TokenStream;
use quote::quote;

#[proc_macro_attribute]
pub fn intercept(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let func = syn::parse_macro_input!(item as syn::ItemFn);
    let func_name = &func.sig.ident;
    let preamble = preamble(Hook::Intercept);

    quote!(
        #func
        mod __kiwi_intercept {
            mod preamble {
                #preamble
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

#[proc_macro_attribute]
pub fn authenticate(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let func = syn::parse_macro_input!(item as syn::ItemFn);
    let func_name = &func.sig.ident;
    let preamble = preamble(Hook::Authenticate);

    quote!(
        #func
        mod __kiwi_authenticate {
            mod preamble {
                #preamble
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

#[derive(Copy, Clone)]
enum Hook {
    Intercept,
    Authenticate,
}

fn preamble(hook: Hook) -> proc_macro2::TokenStream {
    let generated = match hook {
        Hook::Intercept => include_str!("intercept_hook.rs"),
        Hook::Authenticate => include_str!("authenticate_hook.rs"),
    };

    let toks = syn::parse_str::<proc_macro2::TokenStream>(generated)
        .expect("failed to parse wit-bindgen generated code");

    quote! {
        #![allow(missing_docs)]
        #toks

        pub struct Kiwi;
    }
}
