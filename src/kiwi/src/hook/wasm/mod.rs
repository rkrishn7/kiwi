use std::path::Path;

use async_trait::async_trait;
use http::Request as HttpRequest;
use once_cell::sync::Lazy;
use wasi_preview1_component_adapter_provider::WASI_SNAPSHOT_PREVIEW1_REACTOR_ADAPTER;
use wasmtime::component::{Component, InstancePre, Linker, ResourceTable};
use wasmtime::{Config, Engine, Store};
use wasmtime_wasi::{Stdout, WasiCtx, WasiCtxBuilder, WasiImpl, WasiView};
use wasmtime_wasi_http::{WasiHttpCtx, WasiHttpView};

use anyhow::Context;
use wit_component::ComponentEncoder;

use super::authenticate;
use super::authenticate::types::{Authenticate, Outcome};
use super::authenticate::wasm::bindgen::AuthenticateHookPre;
use super::intercept;
use super::intercept::types::Intercept;
use super::intercept::wasm::bindgen::InterceptHookPre;

static ENGINE: Lazy<Engine> = Lazy::new(|| {
    let mut config = Config::new();
    config.wasm_component_model(true);
    config.async_support(true);
    Engine::new(&config).expect("failed to instantiate engine")
});

/// Encode a WebAssembly module into a component suitable for execution in the
/// Kiwi hook runtime.
pub fn encode_component<P: AsRef<Path>>(input: P) -> anyhow::Result<Vec<u8>> {
    let parsed = wat::parse_file(input).context("failed to parse wat")?;

    let mut encoder = ComponentEncoder::default().validate(true).module(&parsed)?;
    encoder = encoder.adapter(
        "wasi_snapshot_preview1",
        WASI_SNAPSHOT_PREVIEW1_REACTOR_ADAPTER,
    )?;

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
    fn table(&mut self) -> &mut ResourceTable {
        &mut self.table
    }

    fn ctx(&mut self) -> &mut WasiCtx {
        &mut self.wasi
    }
}

impl authenticate::wasm::bindgen::kiwi::kiwi::authenticate_types::Host for WasiImpl<Host> {}
impl intercept::wasm::bindgen::kiwi::kiwi::intercept_types::Host for Host {}

pub(super) fn get_linker() -> anyhow::Result<Linker<Host>> {
    let mut linker = Linker::new(&ENGINE);
    wasmtime_wasi::add_to_linker_async(&mut linker)?;
    wasmtime_wasi_http::add_only_http_to_linker_async(&mut linker)?;

    Ok(linker)
}

pub(super) fn create_instance_pre<P: AsRef<Path>>(file: P) -> anyhow::Result<InstancePre<Host>> {
    let linker = get_linker()?;
    let bytes = encode_component(file)?;
    let component = Component::from_binary(&ENGINE, &bytes)?;

    let instance_pre = linker.instantiate_pre(&component)?;

    Ok(instance_pre)
}

pub trait WasmHook {
    /// Create a new instance of the hook from a file
    fn from_file<P: AsRef<Path>>(file: P) -> anyhow::Result<Self>
    where
        Self: Sized;
    /// Path to the WebAssembly module
    fn path(&self) -> &std::path::Path;
}

pub struct WasmAuthenticateHook {
    instance_pre: AuthenticateHookPre<Host>,
    path: std::path::PathBuf,
}

impl WasmHook for WasmAuthenticateHook {
    fn from_file<P: AsRef<Path>>(file: P) -> anyhow::Result<Self> {
        let path = file.as_ref().to_path_buf();
        let instance_pre = create_instance_pre(file)?;
        let instance_pre = AuthenticateHookPre::new(instance_pre)?;

        Ok(Self { instance_pre, path })
    }

    fn path(&self) -> &std::path::Path {
        &self.path
    }
}

pub struct WasmInterceptHook {
    instance_pre: InterceptHookPre<Host>,
    path: std::path::PathBuf,
}

impl WasmHook for WasmInterceptHook {
    fn from_file<P: AsRef<Path>>(file: P) -> anyhow::Result<Self> {
        let path = file.as_ref().to_path_buf();
        let instance_pre = create_instance_pre(file)?;
        let instance_pre = InterceptHookPre::new(instance_pre)?;

        Ok(Self { instance_pre, path })
    }

    fn path(&self) -> &std::path::Path {
        &self.path
    }
}

#[async_trait]
impl Authenticate for WasmAuthenticateHook {
    async fn authenticate(&self, request: HttpRequest<()>) -> anyhow::Result<Outcome> {
        let mut builder = WasiCtxBuilder::new();

        builder.stdout(Stdout);

        let state = Host {
            table: ResourceTable::new(),
            wasi: builder.build(),
            http: WasiHttpCtx::new(),
        };

        let mut store = Store::new(&ENGINE, state);

        let bindings = self.instance_pre.instantiate_async(&mut store).await?;

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
                http: WasiHttpCtx::new(),
            },
        );

        let bindings = self.instance_pre.instantiate_async(&mut store).await?;

        let res = bindings
            .call_intercept(&mut store, &ctx.clone().into())
            .await?;

        Ok(res.into())
    }
}
