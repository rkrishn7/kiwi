mod bindgen;
mod bridge;

use std::path::Path;

use wasmtime::component::{Component, Linker, ResourceTable};
use wasmtime::Store;
use wasmtime::{Config, Engine};
use wasmtime_wasi::preview2::{WasiCtx, WasiCtxBuilder};

use super::types::{Action, Context};
use super::Intercept;

use once_cell::sync::Lazy;

static ENGINE: Lazy<Engine> = Lazy::new(|| {
    let mut config = Config::new();
    config.wasm_component_model(true);
    Engine::new(&config).expect("failed to instantiate engine")
});

struct State {
    table: ResourceTable,
    wasi: WasiCtx,
}

impl bindgen::kiwi::kiwi::intercept_types::Host for State {}

impl wasmtime_wasi::preview2::WasiView for State {
    fn table(&self) -> &ResourceTable {
        &self.table
    }

    fn table_mut(&mut self) -> &mut ResourceTable {
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
pub struct WasmInterceptHook {
    component: Component,
    linker: Linker<State>,
}

impl WasmInterceptHook {
    pub fn from_file(file: impl AsRef<Path>) -> anyhow::Result<Self> {
        let mut linker = Linker::new(&ENGINE);
        bindgen::InterceptHook::add_to_linker(&mut linker, |state: &mut State| state)?;
        wasmtime_wasi::preview2::command::sync::add_to_linker(&mut linker)?;

        Ok(Self {
            component: Component::from_file(&ENGINE, file)?,
            linker,
        })
    }
}

impl Intercept for WasmInterceptHook {
    fn intercept(&self, ctx: &Context) -> anyhow::Result<Action> {
        let mut builder = WasiCtxBuilder::new();

        let mut store = Store::new(
            &ENGINE,
            State {
                table: ResourceTable::new(),
                wasi: builder.build(),
            },
        );

        let (bindings, _) =
            bindgen::InterceptHook::instantiate(&mut store, &self.component, &self.linker)?;

        let res = bindings.call_intercept(&mut store, &ctx.clone().into())?;

        Ok(res.into())
    }
}
