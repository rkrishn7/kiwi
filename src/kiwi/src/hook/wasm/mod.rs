use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use http::Request as HttpRequest;
use once_cell::sync::Lazy;
use wasmtime::component::{Component, InstancePre, Linker, ResourceTable};
use wasmtime::{Config, Engine, Store};
use wasmtime_wasi::preview2::{self, Stdout, WasiCtx, WasiCtxBuilder, WasiView};
use wasmtime_wasi_http::{WasiHttpCtx, WasiHttpView};

use anyhow::Context;
use wit_component::ComponentEncoder;

use super::authenticate;
use super::authenticate::types::{Authenticate, Outcome};
use super::intercept;
use super::intercept::types::Intercept;

const DEFAULT_ADAPTER_PATH: &str = "/etc/kiwi/wasi/wasi_snapshot_preview1.wasm";

static ENGINE: Lazy<Engine> = Lazy::new(|| {
    let mut config = Config::new();
    config.wasm_component_model(true);
    config.async_support(true);
    Engine::new(&config).expect("failed to instantiate engine")
});

/// Encode a WebAssembly module into a component suitable for execution in the
/// Kiwi hook runtime.
pub fn encode_component<P: AsRef<Path>>(input: P, adapter: Option<P>) -> anyhow::Result<Vec<u8>> {
    let parsed = wat::parse_file(input).context("failed to parse wat")?;
    let adapter_path: &Path = adapter
        .as_ref()
        .map(|p| p.as_ref())
        .unwrap_or(Path::new(DEFAULT_ADAPTER_PATH));

    let adapter = wat::parse_file(adapter_path).context("failed to parse adapter")?;

    let mut encoder = ComponentEncoder::default().validate(true).module(&parsed)?;
    encoder = encoder.adapter("wasi_snapshot_preview1", &adapter)?;

    encoder.encode().context("failed to encode component")
}

pub struct Host {
    table: ResourceTable,
    wasi: WasiCtx,
    http: WasiHttpCtx,
}

impl WasiHttpView for Host {
    fn ctx(&mut self) -> &mut WasiHttpCtx {
        &mut self.http
    }

    fn table(&mut self) -> &mut ResourceTable {
        &mut self.table
    }
}

impl WasiView for Host {
    fn table(&self) -> &ResourceTable {
        &self.table
    }

    fn table_mut(&mut self) -> &mut ResourceTable {
        &mut self.table
    }

    fn ctx(&self) -> &WasiCtx {
        &self.wasi
    }

    fn ctx_mut(&mut self) -> &mut WasiCtx {
        &mut self.wasi
    }
}

impl authenticate::wasm::bindgen::kiwi::kiwi::authenticate_types::Host for Host {}
impl intercept::wasm::bindgen::kiwi::kiwi::intercept_types::Host for Host {}

pub(super) fn get_linker(typ: WasmHookType) -> anyhow::Result<Linker<Host>> {
    let mut linker = Linker::new(&ENGINE);
    preview2::command::add_to_linker(&mut linker)?;

    if typ == WasmHookType::Authenticate {
        wasmtime_wasi_http::proxy::add_only_http_to_linker(&mut linker)?;
        authenticate::wasm::bindgen::AuthenticateHook::add_to_linker(
            &mut linker,
            |state: &mut Host| state,
        )?;
    } else {
        intercept::wasm::bindgen::InterceptHook::add_to_linker(&mut linker, |state: &mut Host| {
            state
        })?;
    }

    Ok(linker)
}

pub(super) fn create_instance_pre<P: AsRef<Path>>(
    typ: WasmHookType,
    file: P,
    adapter: Option<P>,
) -> anyhow::Result<InstancePre<Host>> {
    let linker = get_linker(typ)?;
    let bytes = encode_component(file, adapter)?;
    let component = Component::from_binary(&ENGINE, &bytes)?;

    let instance_pre = linker.instantiate_pre(&component)?;

    Ok(instance_pre)
}

#[derive(Clone)]
pub struct WasmAuthenticateHook {
    instance_pre: Arc<InstancePre<Host>>,
}

impl WasmAuthenticateHook {
    pub fn from_file<P: AsRef<Path>>(file: P, adapter: Option<P>) -> anyhow::Result<Self> {
        let instance_pre = create_instance_pre(WasmHookType::Authenticate, file, adapter)?;

        Ok(Self {
            instance_pre: Arc::new(instance_pre),
        })
    }
}

#[derive(Clone)]
pub struct WasmInterceptHook {
    instance_pre: Arc<InstancePre<Host>>,
}

impl WasmInterceptHook {
    pub fn from_file<P: AsRef<Path>>(file: P, adapter: Option<P>) -> anyhow::Result<Self> {
        let instance_pre = create_instance_pre(WasmHookType::Intercept, file, adapter)?;

        Ok(Self {
            instance_pre: Arc::new(instance_pre),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WasmHookType {
    Authenticate,
    Intercept,
}

#[async_trait]
impl Authenticate for WasmAuthenticateHook {
    async fn authenticate(&self, request: HttpRequest<()>) -> anyhow::Result<Outcome> {
        let mut builder = WasiCtxBuilder::new();

        builder.stdout(Stdout);

        let state = Host {
            table: ResourceTable::new(),
            wasi: builder.build(),
            http: WasiHttpCtx,
        };

        let mut store = Store::new(&ENGINE, state);

        let (bindings, _) = authenticate::wasm::bindgen::AuthenticateHook::instantiate_pre(
            &mut store,
            &self.instance_pre,
        )
        .await?;

        let res = bindings
            .call_authenticate(&mut store, &request.into())
            .await?;

        Ok(res.into())
    }
}

#[async_trait]
impl Intercept for WasmInterceptHook {
    async fn intercept(
        &self,
        ctx: &super::intercept::types::Context,
    ) -> anyhow::Result<super::intercept::types::Action> {
        let mut builder = WasiCtxBuilder::new();

        let mut store = Store::new(
            &ENGINE,
            Host {
                table: ResourceTable::new(),
                wasi: builder.build(),
                http: WasiHttpCtx,
            },
        );

        let (bindings, _) = intercept::wasm::bindgen::InterceptHook::instantiate_pre(
            &mut store,
            &self.instance_pre,
        )
        .await?;

        let res = bindings
            .call_intercept(&mut store, &ctx.clone().into())
            .await?;

        Ok(res.into())
    }
}
