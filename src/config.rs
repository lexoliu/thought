use crate::error::{Error, Result};
use crate::utils::read_to_string;
use serde::{Deserialize, Serialize};
use std::path::Path;
use whoami::realname;
#[derive(Debug, Deserialize, Serialize)]
pub struct Config {
    name: String,
    owner: String,
    template: String,
}

impl Config {
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self> {
        let config = read_to_string(path)?;
        toml::from_str(&config).map_err(Error::InvalidConfig)
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn owner(&self) -> &str {
        &self.owner
    }

    pub fn template(&self) -> &str {
        &self.template
    }

    pub fn export(&self) -> String {
        // Serialization for config never fail, so that we can use `unwrap`
        toml::to_string_pretty(&self).unwrap()
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            name: "Thought".to_string(),
            owner: realname(),
            template: "zenflow".to_string(),
        }
    }
}
