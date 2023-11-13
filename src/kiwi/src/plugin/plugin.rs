use super::types::{Action, Context};

pub trait Plugin {
    fn call(&self, context: &Context) -> anyhow::Result<Action>;
}
