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

            impl From<self::preamble::kiwi::kiwi::intercept_types::Context> for ::kiwi_sdk::types::intercept::Context {
                fn from(value: self::preamble::kiwi::kiwi::intercept_types::Context) -> Self {
                    Self {
                        auth: value.auth.map(|raw| {
                            ::kiwi_sdk::types::intercept::AuthCtx {
                                raw,
                            }
                        }),
                        connection: value.connection.into(),
                        event: value.event.into(),
                    }
                }
            }

            impl From<self::preamble::kiwi::kiwi::intercept_types::EventCtx> for ::kiwi_sdk::types::intercept::EventCtx {
                fn from(value: self::preamble::kiwi::kiwi::intercept_types::EventCtx) -> Self {
                    match value {
                        self::preamble::kiwi::kiwi::intercept_types::EventCtx::Kafka(ctx) => Self::Kafka(ctx.into()),
                        self::preamble::kiwi::kiwi::intercept_types::EventCtx::Counter(ctx) => Self::Counter(ctx.into()),
                    }
                }
            }

            impl From<self::preamble::kiwi::kiwi::intercept_types::CounterEventCtx> for ::kiwi_sdk::types::intercept::CounterEventCtx {
                fn from(value: self::preamble::kiwi::kiwi::intercept_types::CounterEventCtx) -> Self {
                    Self {
                        source_id: value.source_id,
                        count: value.count,
                    }
                }
            }

            impl From<self::preamble::kiwi::kiwi::intercept_types::KafkaEventCtx> for ::kiwi_sdk::types::intercept::KafkaEventCtx {
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

            impl From<self::preamble::kiwi::kiwi::intercept_types::ConnectionCtx> for ::kiwi_sdk::types::intercept::ConnectionCtx {
                fn from(value: self::preamble::kiwi::kiwi::intercept_types::ConnectionCtx) -> Self {
                    match value {
                        self::preamble::kiwi::kiwi::intercept_types::ConnectionCtx::Websocket(ctx) => Self::WebSocket(ctx.into()),
                    }
                }
            }

            impl From<self::preamble::kiwi::kiwi::intercept_types::Websocket> for ::kiwi_sdk::types::intercept::WebSocketConnectionCtx {
                fn from(value: self::preamble::kiwi::kiwi::intercept_types::Websocket) -> Self {
                    Self {
                        addr: value.addr,
                    }
                }
            }

            impl From<::kiwi_sdk::types::intercept::Action> for self::preamble::kiwi::kiwi::intercept_types::Action {
                fn from(value: ::kiwi_sdk::types::intercept::Action) -> Self {
                    match value {
                        ::kiwi_sdk::types::intercept::Action::Forward => Self::Forward,
                        ::kiwi_sdk::types::intercept::Action::Discard => Self::Discard,
                        ::kiwi_sdk::types::intercept::Action::Transform(payload) => Self::Transform(payload.into()),
                    }
                }
            }

            impl From<::kiwi_sdk::types::intercept::TransformedPayload> for self::preamble::kiwi::kiwi::intercept_types::TransformedPayload {
                fn from(value: ::kiwi_sdk::types::intercept::TransformedPayload) -> Self {
                    match value {
                        ::kiwi_sdk::types::intercept::TransformedPayload::Kafka(payload) => Self::Kafka(payload),
                        ::kiwi_sdk::types::intercept::TransformedPayload::Counter(count) => Self::Counter(count),
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
            pub mod preamble {
                #preamble
            }

            impl preamble::Guest for preamble::Kiwi {
                fn authenticate(incoming: self::preamble::wasi::http::types::IncomingRequest) -> self::preamble::kiwi::kiwi::authenticate_types::Outcome {
                    super::#func_name(incoming).into()
                }
            }

            impl From<::kiwi_sdk::types::authenticate::Outcome> for self::preamble::kiwi::kiwi::authenticate_types::Outcome {
                fn from(value: ::kiwi_sdk::types::authenticate::Outcome) -> Self {
                    match value {
                        ::kiwi_sdk::types::authenticate::Outcome::Authenticate => Self::Authenticate,
                        ::kiwi_sdk::types::authenticate::Outcome::Reject => Self::Reject,
                        ::kiwi_sdk::types::authenticate::Outcome::WithContext(payload) => Self::WithContext(payload),
                    }
                }
            }

        }
    )
        .into()
}

#[proc_macro]
pub fn use_wasi_http_types(item: TokenStream) -> TokenStream {
    let name = syn::parse_macro_input!(item as syn::Ident);

    quote! {
        use __kiwi_authenticate::preamble::wasi::http::types as #name;
    }
    .into()
}

#[proc_macro]
pub fn make_http_request_fn(item: TokenStream) -> TokenStream {
    let name = syn::parse_macro_input!(item as syn::Ident);

    quote! {
        fn #name(
            method: __kiwi_authenticate::preamble::wasi::http::types::Method,
            scheme: __kiwi_authenticate::preamble::wasi::http::types::Scheme,
            authority: &str,
            path_with_query: &str,
            body: Option<&[u8]>,
            additional_headers: Option<&[(String, Vec<u8>)]>,
        ) -> anyhow::Result<::kiwi_sdk::types::http::Response> {
            fn header_val(v: &str) -> Vec<u8> {
                v.to_string().into_bytes()
            }
            let headers = __kiwi_authenticate::preamble::wasi::http::types::Headers::from_list(
                &[
                    &[
                        ("User-agent".to_string(), header_val("WASI-HTTP/0.0.1")),
                        ("Content-type".to_string(), header_val("application/json")),
                    ],
                    additional_headers.unwrap_or(&[]),
                ]
                .concat(),
            )?;

            let request = __kiwi_authenticate::preamble::wasi::http::types::OutgoingRequest::new(headers);

            request
                .set_method(&method)
                .map_err(|()| anyhow::anyhow!("failed to set method"))?;
            request
                .set_scheme(Some(&scheme))
                .map_err(|()| anyhow::anyhow!("failed to set scheme"))?;
            request
                .set_authority(Some(authority))
                .map_err(|()| anyhow::anyhow!("failed to set authority"))?;
            request
                .set_path_with_query(Some(&path_with_query))
                .map_err(|()| anyhow::anyhow!("failed to set path_with_query"))?;

            let outgoing_body = request
                .body()
                .map_err(|_| anyhow::anyhow!("outgoing request write failed"))?;

            if let Some(mut buf) = body {
                let request_body = outgoing_body
                    .write()
                    .map_err(|_| anyhow::anyhow!("outgoing request write failed"))?;

                let pollable = request_body.subscribe();
                while !buf.is_empty() {
                    pollable.block();

                    let permit = match request_body.check_write() {
                        Ok(n) => n,
                        Err(_) => anyhow::bail!("output stream error"),
                    };

                    let len = buf.len().min(permit as usize);
                    let (chunk, rest) = buf.split_at(len);
                    buf = rest;

                    match request_body.write(chunk) {
                        Err(_) => anyhow::bail!("output stream error"),
                        _ => {}
                    }
                }

                match request_body.flush() {
                    Err(_) => anyhow::bail!("output stream error"),
                    _ => {}
                }

                pollable.block();

                match request_body.check_write() {
                    Ok(_) => {}
                    Err(_) => anyhow::bail!("output stream error"),
                };
            }

            let future_response = __kiwi_authenticate::preamble::wasi::http::outgoing_handler::handle(request, None)?;

            __kiwi_authenticate::preamble::wasi::http::types::OutgoingBody::finish(outgoing_body, None)?;

            let incoming_response = match future_response.get() {
                Some(result) => result.map_err(|()| anyhow::anyhow!("response already taken"))?,
                None => {
                    let pollable = future_response.subscribe();
                    pollable.block();
                    future_response
                        .get()
                        .expect("incoming response available")
                        .map_err(|()| anyhow::anyhow!("response already taken"))?
                }
            }?;

            drop(future_response);

            let status = incoming_response.status();

            let headers_handle = incoming_response.headers();
            let headers = headers_handle.entries();
            drop(headers_handle);

            let incoming_body = incoming_response
                .consume()
                .map_err(|()| anyhow::anyhow!("incoming response has no body stream"))?;

            drop(incoming_response);

            let input_stream = incoming_body.stream().unwrap();
            let input_stream_pollable = input_stream.subscribe();

            let mut body = Vec::new();
            loop {
                input_stream_pollable.block();

                let mut body_chunk = match input_stream.read(1024 * 1024) {
                    Ok(c) => c,
                    Err(__kiwi_authenticate::preamble::wasi::io::streams::StreamError::Closed) => break,
                    Err(e) => Err(anyhow::anyhow!("input_stream read failed: {e:?}"))?,
                };

                if !body_chunk.is_empty() {
                    body.append(&mut body_chunk);
                }
            }

            Ok(::kiwi_sdk::types::http::Response {
                status,
                headers,
                body,
            })
        }
    }
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
