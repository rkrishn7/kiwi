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
                        ::kiwi_sdk::types::intercept::Action::Transform(payload) => Self::Transform(payload),
                    }
                }
            }
            
        }
    )
        .into()
}

#[derive(Copy, Clone)]
enum Hook {
    Intercept,
}

fn preamble(hook: Hook) -> proc_macro2::TokenStream {
    let export_decl = match hook {
        Hook::Intercept => quote!(world: Kiwi),
    };
    let world = match hook {
        Hook::Intercept => quote!("intercept-hook"),
    };

    let inline = format!(
        "
{}
{}
{}
",
        include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/wit/world.wit")),
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/wit/intercept-types.wit"
        )),
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/wit/authenticate-types.wit"
        ))
    );

    quote! {
        #![allow(missing_docs)]
        ::kiwi_sdk::wit_bindgen::generate!({
            world: #world,
            inline: #inline,
            runtime_path: "::kiwi_sdk::wit_bindgen::rt",
            exports: {
                #export_decl
            }
        });
        pub struct Kiwi;
    }
}
