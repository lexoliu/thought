use crate::utils::{read_to_string, Error, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;
#[derive(Debug, Deserialize, Serialize)]
pub struct Config {
    pub name: String,
    pub author: String,
    pub template: String,
}

impl Config {
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self> {
        let config = read_to_string(path)?;
        toml::from_str(&config).map_err(Error::InvalidConfig)
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            name: "Thought".to_string(),
            author: "?".to_string(),
            template: "Thought".to_string(),
        }
    }
}
