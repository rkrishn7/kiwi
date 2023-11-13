use std::{fs::File, io::Read};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    pub sources: Sources,
    pub plugins: Option<Plugins>,
    pub server: Server,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Sources {
    pub kafka: Kafka,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Kafka {
    #[serde(default = "Kafka::default_group_prefix")]
    pub group_prefix: String,
    pub bootstrap_servers: Vec<String>,
    pub topics: Vec<Topic>,
}

impl Kafka {
    fn default_group_prefix() -> String {
        "kiwi-".into()
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Plugins {
    pub pre_forward: Option<String>,
}

/// Topic configuration
#[derive(Debug, Clone, Deserialize, Serialize, Hash, PartialEq, Eq)]
pub struct Topic {
    pub name: String,
}

/// Server configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Server {
    pub address: String,
}

impl Config {
    pub fn parse(path: &str) -> Result<Self, anyhow::Error> {
        let mut file = File::open(path)?;
        let mut contents = String::new();
        file.read_to_string(&mut contents)?;

        let config = serde_yaml::from_str::<'_, Config>(&contents)?;

        Ok(config)
    }
}
