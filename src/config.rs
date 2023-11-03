#[derive(Debug, Clone)]
pub struct Config {
    pub brokers_list: String,
    pub topics: Vec<String>,
}

impl Config {
    pub fn parse() -> Self {
        Self {
            brokers_list: String::from("test"),
            topics: vec![String::from("test")],
        }
    }

    pub fn topics(&self) -> Vec<&str> {
        self.topics.iter().map(AsRef::as_ref).collect()
    }
}
