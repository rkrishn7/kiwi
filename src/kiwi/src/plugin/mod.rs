pub mod types;
mod wasm;
pub use wasm::WasmPlugin;

use types::{Action, Context};

pub trait Plugin {
    fn call(&self, context: &Context) -> anyhow::Result<Action>;
}
