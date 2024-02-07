mod bindgen;
mod bridge;

use std::path::Path;

use wasmtime::component::{Component, Linker, ResourceTable};
use wasmtime::Store;
use wasmtime::{Config, Engine};
use wasmtime_wasi::preview2::{self, Stdout, WasiCtx, WasiCtxBuilder};
use wasmtime_wasi_http::bindings::http::types::IncomingRequest;
use wasmtime_wasi_http::{WasiHttpCtx, WasiHttpView};

use crate::hook::wasm::encode_component;

use super::types::Outcome;
use super::Authenticate;

use once_cell::sync::Lazy;
use tokio_tungstenite::tungstenite::http::Request as HttpRequest;

static ENGINE: Lazy<Engine> = Lazy::new(|| {
    let mut config = Config::new();
    config.wasm_component_model(true);
    config.async_support(true);
    Engine::new(&config).expect("failed to instantiate engine")
});

struct State {
    table: ResourceTable,
    wasi: WasiCtx,
    http: WasiHttpCtx,
}

impl bindgen::kiwi::kiwi::authenticate_types::Host for State {}

impl wasmtime_wasi_http::WasiHttpView for State {
    fn ctx(&mut self) -> &mut WasiHttpCtx {
        &mut self.http
    }

    fn table(&mut self) -> &mut ResourceTable {
        &mut self.table
    }
}

impl wasmtime_wasi::preview2::WasiView for State {
    fn table(&self) -> &wasmtime_wasi::preview2::ResourceTable {
        &self.table
    }

    fn table_mut(&mut self) -> &mut wasmtime_wasi::preview2::ResourceTable {
        &mut self.table
    }

    fn ctx(&self) -> &wasmtime_wasi::preview2::WasiCtx {
        &self.wasi
    }

    fn ctx_mut(&mut self) -> &mut wasmtime_wasi::preview2::WasiCtx {
        &mut self.wasi
    }
}

#[derive(Clone)]
pub struct WasmAuthenticateHook {
    component: Component,
    linker: Linker<State>,
}

impl WasmAuthenticateHook {
    pub fn from_file<P: AsRef<Path>>(file: P, adapter: Option<P>) -> anyhow::Result<Self> {
        let mut linker = Linker::new(&ENGINE);
        preview2::command::add_to_linker(&mut linker)?;
        wasmtime_wasi_http::proxy::add_only_http_to_linker(&mut linker)?;
        bindgen::AuthenticateHook::add_to_linker(&mut linker, |state: &mut State| state)?;

        let bytes = encode_component(file, adapter)?;

        Ok(Self {
            component: Component::from_binary(&ENGINE, &bytes)?,
            linker,
        })
    }
}

#[async_trait::async_trait]
impl Authenticate for WasmAuthenticateHook {
    async fn authenticate(&self, request: &HttpRequest<()>) -> anyhow::Result<Outcome> {
        let mut builder = WasiCtxBuilder::new();

        builder.stdout(Stdout);
        builder.allow_ip_name_lookup(false);
        builder.allow_tcp(false);
        builder.socket_addr_check(|_, _| false);

        let mut state = State {
            table: ResourceTable::new(),
            wasi: builder.build(),
            http: WasiHttpCtx,
        };

        // state
        //     .table()
        //     .push(wasmtime_wasi_http::types::HostOutgoingRequest {
        //         path_with_query: Some("/healthz".into()),
        //         authority: Some("api.joinfound.com".into()),
        //         method: wasmtime_wasi_http::bindings::http::types::Method::Get,
        //         headers: wasmtime_wasi_http::types::FieldMap::new(),
        //         scheme: Some(wasmtime_wasi_http::bindings::http::types::Scheme::Https),
        //         body: None,
        //     })?;

        let (parts, _) = request.clone().into_parts();

        let request = IncomingRequest::new(&mut state, parts, None);

        let resource = state.table().push(request)?;

        let mut store = Store::new(&ENGINE, state);

        let (bindings, _) =
            bindgen::AuthenticateHook::instantiate_async(&mut store, &self.component, &self.linker)
                .await?;

        let res = bindings.call_authenticate(&mut store, resource).await?;

        Ok(res.into())
    }
}
