use std::{collections::HashMap, fs::File, io::Read};

use serde::{Deserialize, Serialize};

type ConsumerConfig = HashMap<String, String>;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    pub consumer: Consumer,
    pub topics: Vec<Topic>,
    pub server: Server,
}

/// Topic configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Topic {
    pub name: String,
}

/// Consumer configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Consumer {
    pub group_prefix: String,
    pub config: ConsumerConfig,
}

/// Server configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Server {
    address: String,
}

impl Config {
    pub fn parse(path: &str) -> Result<Self, anyhow::Error> {
        let mut file = File::open(path)?;
        let mut contents = String::new();
        file.read_to_string(&mut contents)?;

        let config = serde_yaml::from_str::<'_, Config>(&contents)?;

        Ok(config)
    }

    pub fn topics(&self) -> &[Topic] {
        &self.topics[..]
    }

    pub fn server_addr(&self) -> &str {
        self.server.address.as_str()
    }

    pub fn consumer(&self) -> &Consumer {
        &self.consumer
    }

    pub fn consumer_config_mut(&mut self) -> &mut ConsumerConfig {
        &mut self.consumer.config
    }

    pub fn consumer_group_prefix(&self) -> &str {
        &self.consumer.group_prefix
    }
}
