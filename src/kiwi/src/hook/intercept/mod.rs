pub mod types;
pub mod wasm;

use types::{Action, Context};

pub trait Intercept {
    fn intercept(&self, context: &Context) -> anyhow::Result<Action>;
}
