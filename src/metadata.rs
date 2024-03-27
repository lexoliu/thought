use std::path::Path;

use crate::{
    utils::{create_file, read_to_string, write_file},
    Error, Result,
};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CategoryMetadata {
    #[serde(with = "time::serde::rfc3339")]
    created: OffsetDateTime,
    name: String,
    #[serde(default)]
    description: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArticleMetadata {
    #[serde(with = "time::serde::rfc3339")]
    created: OffsetDateTime,
    #[serde(default)]
    tags: Vec<String>,
    author: String,
}

impl ArticleMetadata {
    pub fn new(author: impl Into<String>) -> Self {
        Self {
            created: OffsetDateTime::now_utc(),
            author: author.into(),
            tags: Vec::new(),
        }
    }

    pub fn create(path: impl AsRef<Path>, author: impl Into<String>) -> Result<Self> {
        let metadata = Self::new(author);
        create_file(path, metadata.export())?;
        Ok(metadata)
    }

    pub const fn created(&self) -> OffsetDateTime {
        self.created
    }

    pub fn author(&self) -> &str {
        &self.author
    }

    pub fn set_author(&mut self, author: impl Into<String>) {
        self.author = author.into();
    }

    pub fn tags(&self) -> &[String] {
        &self.tags
    }

    pub fn add_tag(&mut self, tag: impl Into<String>) {
        self.tags.push(tag.into());
    }
}

impl CategoryMetadata {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            created: OffsetDateTime::now_utc(),
            name: name.into(),
            description: String::new(),
        }
    }

    pub fn create(path: impl AsRef<Path>, name: impl Into<String>) -> Result<Self> {
        let metadata = Self::new(name);
        create_file(path, metadata.export())?;

        Ok(metadata)
    }

    pub const fn created(&self) -> OffsetDateTime {
        self.created
    }
}

macro_rules! convenience {
    ($($ty:ty),*) => {
        $(
        impl $ty {

            pub fn open(path: impl AsRef<Path>) -> Result<Self> {
                let metadata = read_to_string(path)?;
                toml::from_str(&metadata).map_err(Error::InvalidMetadata)
            }


            pub fn export(&self) -> String {
                // Serialization for config never fail, so that we can use `unwrap` silently.
                toml::to_string_pretty(&self).unwrap()
            }

            pub fn save(&self, path: impl AsRef<Path>) -> std::io::Result<()> {
                write_file(path,self.export())?;
                Ok(())
            }
        })*
    };
}

convenience!(ArticleMetadata, CategoryMetadata);
