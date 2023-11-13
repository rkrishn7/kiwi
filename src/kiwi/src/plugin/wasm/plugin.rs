use std::path::Path;

use wasmtime::component::{Component, Linker};
use wasmtime::Store;
use wasmtime::{Config, Engine};
use wasmtime_wasi::preview2::{Table, WasiCtx, WasiCtxBuilder};

use crate::plugin::types::{Action, Context};
use crate::plugin::Plugin;

use super::bindgen;
use once_cell::sync::Lazy;

static ENGINE: Lazy<Engine> = Lazy::new(|| {
    let mut config = Config::new();
    config.wasm_component_model(true);
    Engine::new(&config).expect("failed to instantiate engine")
});

struct WasmPluginState {
    table: Table,
    wasi: WasiCtx,
}

impl bindgen::kiwi::kiwi::types::Host for WasmPluginState {}

impl wasmtime_wasi::preview2::WasiView for WasmPluginState {
    fn table(&self) -> &wasmtime_wasi::preview2::Table {
        &self.table
    }

    fn table_mut(&mut self) -> &mut wasmtime_wasi::preview2::Table {
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
pub struct WasmPlugin {
    component: Component,
}

impl WasmPlugin {
    pub fn from_file(file: impl AsRef<Path>) -> anyhow::Result<Self> {
        Ok(Self {
            component: Component::from_file(&ENGINE, file)?,
        })
    }
}

impl Plugin for WasmPlugin {
    fn call(&self, ctx: &Context) -> anyhow::Result<Action> {
        let mut linker = Linker::new(&ENGINE);
        bindgen::Plugin::add_to_linker(&mut linker, |state: &mut WasmPluginState| state)?;
        wasmtime_wasi::preview2::command::sync::add_to_linker(&mut linker)?;

        let mut builder = WasiCtxBuilder::new();

        let mut store = Store::new(
            &ENGINE,
            WasmPluginState {
                table: Table::new(),
                wasi: builder.build(),
            },
        );

        let (bindings, _) = bindgen::Plugin::instantiate(&mut store, &self.component, &linker)?;

        let res = bindings.call_execute(&mut store, &ctx.clone().into())?;

        Ok(res.into())
    }
}
