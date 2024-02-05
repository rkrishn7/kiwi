mod bindgen;
mod bridge;

use std::path::Path;

use wasmtime::component::{Component, Linker, ResourceTable};
use wasmtime::Store;
use wasmtime::{Config, Engine};
use wasmtime_wasi::preview2::{WasiCtx, WasiCtxBuilder};

use super::types::Outcome;
use super::Authenticate;

use once_cell::sync::Lazy;
use tokio_tungstenite::tungstenite::http::Request as HttpRequest;

static ENGINE: Lazy<Engine> = Lazy::new(|| {
    let mut config = Config::new();
    config.wasm_component_model(true);
    Engine::new(&config).expect("failed to instantiate engine")
});

struct State {
    table: ResourceTable,
    wasi: WasiCtx,
}

impl bindgen::kiwi::kiwi::authenticate_types::Host for State {}

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
    pub fn from_file(file: impl AsRef<Path>) -> anyhow::Result<Self> {
        let mut linker = Linker::new(&ENGINE);
        bindgen::AuthenticateHook::add_to_linker(&mut linker, |state: &mut State| state)?;
        wasmtime_wasi::preview2::command::sync::add_to_linker(&mut linker)?;

        Ok(Self {
            component: Component::from_file(&ENGINE, file)?,
            linker,
        })
    }
}

impl Authenticate for WasmAuthenticateHook {
    fn authenticate(&self, request: &HttpRequest<()>) -> anyhow::Result<Outcome> {
        let mut builder = WasiCtxBuilder::new();

        let mut store = Store::new(
            &ENGINE,
            State {
                table: ResourceTable::new(),
                wasi: builder.build(),
            },
        );

        let (bindings, _) =
            bindgen::AuthenticateHook::instantiate(&mut store, &self.component, &self.linker)?;

        let res = bindings.call_authenticate(&mut store, &request.into())?;

        Ok(res.into())
    }
}
