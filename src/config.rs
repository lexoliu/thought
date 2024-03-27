use crate::error::{Error, Result};
use crate::utils::{read_to_string, write_file};
use serde::{Deserialize, Serialize};

use std::path::Path;
use whoami::realname;
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    title: String,
    owner: String,
    template: String,
}

impl Config {
    pub fn new(template: impl Into<String>) -> Self {
        Self {
            title: "Thought".to_string(),
            owner: realname(),
            template: template.into(),
        }
    }
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self> {
        let config = read_to_string(path)?;
        toml::from_str(&config).map_err(Error::InvalidConfig)
    }

    pub fn title(&self) -> &str {
        &self.title
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

    pub fn save(&self, path: impl AsRef<Path>) -> Result<()> {
        write_file(path, self.export())?;
        Ok(())
    }
}
