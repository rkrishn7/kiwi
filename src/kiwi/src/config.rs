use std::{fs::File, io::Read};

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub sources: Sources,
    pub plugins: Option<Plugins>,
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
pub struct Plugins {
    pub pre_forward: Option<String>,
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
    pub authentication: Option<Authentication>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Authentication {
    pub jwt: JwtAuthentication,
}

#[derive(Debug, Clone, Deserialize)]
pub struct JwtAuthentication {
    pub algorithm: JwtAlgorithmType,
    pub pem_file: String,
}

/// Supported JWT algorithms
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum JwtAlgorithmType {
    Hs256,
    Rs256,
}

impl From<JwtAlgorithmType> for jwt::AlgorithmType {
    fn from(value: JwtAlgorithmType) -> Self {
        match value {
            JwtAlgorithmType::Hs256 => jwt::AlgorithmType::Hs256,
            JwtAlgorithmType::Rs256 => jwt::AlgorithmType::Rs256,
        }
    }
}

impl Config {
    pub fn parse(path: &str) -> Result<Self, anyhow::Error> {
        let mut file = File::open(path)?;
        let mut contents = String::new();
        file.read_to_string(&mut contents)?;

        Self::from_str(&contents)
    }

    fn from_str(contents: &str) -> Result<Self, anyhow::Error> {
        let config = serde_yaml::from_str::<'_, Config>(&contents)?;

        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_plugins() {
        // Ensure we can parse a config that includes the pre_forward plugin
        let config = "
        plugins:
            pre_forward: ./test.wasm
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

        assert!(config.plugins.is_some());
        assert_eq!(
            config.plugins.unwrap().pre_forward,
            Some("./test.wasm".into())
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

        assert!(config.plugins.is_none());
    }

    #[test]
    fn test_authentication() {
        // Ensure we can parse a config that does not include any authentication
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

        assert!(config.server.authentication.is_none());

        // Ensure we can parse a config that includes JWT authentication
        let config = "
        sources:
            kafka:
                bootstrap_servers:
                    - localhost:9092
                topics:
                    - name: test
        server:
            address: '127.0.0.1:8000'
            authentication:
                jwt:
                    algorithm: HS256
                    pem_file: ./test.pem
        ";

        let config = Config::from_str(config).unwrap();

        let authentication = config.server.authentication.unwrap();

        assert_eq!(authentication.jwt.algorithm, JwtAlgorithmType::Hs256);
        assert_eq!(authentication.jwt.pem_file, "./test.pem".to_string());
    }
}
