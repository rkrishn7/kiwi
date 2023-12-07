use std::{fs::File, io::Read};

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub sources: Sources,
    pub hooks: Option<Hooks>,
    pub server: Server,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Sources {
    pub kafka: Kafka,
}

#[derive(Debug, Clone, Deserialize)]
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

#[derive(Debug, Clone, Deserialize)]
pub struct Hooks {
    pub intercept: Option<String>,
    pub authenticate: Option<String>,
}

/// Topic configuration
#[derive(Debug, Clone, Deserialize, Hash, PartialEq, Eq)]
pub struct Topic {
    pub name: String,
}

/// Server configuration
#[derive(Debug, Clone, Deserialize)]
pub struct Server {
    pub address: String,
}

impl Config {
    pub fn parse(path: &str) -> Result<Self, anyhow::Error> {
        let mut file = File::open(path)?;
        let mut contents = String::new();
        file.read_to_string(&mut contents)?;

        Self::from_str(&contents)
    }

    fn from_str(contents: &str) -> Result<Self, anyhow::Error> {
        let config = serde_yaml::from_str::<'_, Config>(contents)?;

        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_hooks() {
        // Ensure we can parse a config that includes hooks
        let config = "
        hooks:
            intercept: ./intercept.wasm
            authenticate: ./auth.wasm
        sources:
            kafka:
                bootstrap_servers:
                    - localhost:9092
                topics:
                    - name: test
        server:
            address: '127.0.0.1:8000'
        ";

        let config = Config::from_str(config).unwrap();

        assert!(config.hooks.is_some());
        assert_eq!(
            config.hooks.clone().unwrap().intercept,
            Some("./intercept.wasm".into())
        );
        assert_eq!(
            config.hooks.unwrap().authenticate,
            Some("./auth.wasm".into())
        );

        // Ensure we can parse a config that does not include any plugins
        let config = "
        sources:
            kafka:
                bootstrap_servers:
                    - localhost:9092
                topics:
                    - name: test
        server:
            address: '127.0.0.1:8000'
        ";

        let config = Config::from_str(config).unwrap();

        assert!(config.hooks.is_none());
    }
}
