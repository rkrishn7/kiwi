use std::path::Path;

use wasmtime::component::{Component, Linker};
use wasmtime::Store;
use wasmtime::{Config, Engine};
use wasmtime_wasi::preview2::{Table, WasiCtx, WasiCtxBuilder};

use super::bindgen;
use once_cell::sync::Lazy;

static ENGINE: Lazy<Engine> = Lazy::new(|| {
    let mut config = Config::new();
    config.wasm_component_model(true);
    Engine::new(&config).expect("failed to instantiate engine")
});

struct PluginState {
    table: Table,
    wasi: WasiCtx,
}

impl bindgen::kiwi::kiwi::types::Host for PluginState {}

impl wasmtime_wasi::preview2::WasiView for PluginState {
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
pub struct Plugin {
    component: Component,
}

impl std::fmt::Debug for Plugin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FilterPlugin")
            .field("component", &"<not visible>")
            .finish()
    }
}

impl Plugin {
    pub fn from_file(file: impl AsRef<Path>) -> anyhow::Result<Self> {
        Ok(Self {
            component: Component::from_file(&ENGINE, file)?,
        })
    }
}

impl Plugin {
    pub fn call(
        &self,
        ctx: impl Into<bindgen::kiwi::kiwi::types::Context>,
    ) -> anyhow::Result<bindgen::kiwi::kiwi::types::Action> {
        let mut linker = Linker::new(&ENGINE);
        bindgen::Plugin::add_to_linker(&mut linker, |state: &mut PluginState| state)?;
        wasmtime_wasi::preview2::command::sync::add_to_linker(&mut linker)?;

        let mut builder = WasiCtxBuilder::new();

        let mut store = Store::new(
            &ENGINE,
            PluginState {
                table: Table::new(),
                wasi: builder.build(),
            },
        );

        let (bindings, _) = bindgen::Plugin::instantiate(&mut store, &self.component, &linker)?;

        let res = bindings.call_execute(&mut store, &ctx.into())?;

        Ok(res)
    }
}
