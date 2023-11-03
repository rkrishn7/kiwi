use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Command {
    Subscribe { topics: Vec<String> },
    Unsubscribe { topics: Vec<String> },
}
