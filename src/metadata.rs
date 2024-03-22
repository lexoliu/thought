use std::{fs::File, io::Write, path::Path};

use crate::{utils::read_to_string, Error, Result};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

#[derive(Debug, Deserialize, Serialize)]
pub struct CategoryMetadata {
    #[serde(with = "time::serde::rfc3339")]
    created: OffsetDateTime,
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

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "type")]

pub enum Metadata {
    Artcile(ArticleMetadata),
    Category(CategoryMetadata),
}

impl Metadata {
    pub const fn created(&self) -> OffsetDateTime {
        match self {
            Metadata::Artcile(metadata) => metadata.created(),
            Metadata::Category(metadata) => metadata.created(),
        }
    }
}

impl ArticleMetadata {
    pub fn new(author: impl Into<String>) -> Self {
        Self {
            created: OffsetDateTime::now_utc(),
            author: author.into(),
            tags: Vec::new(),
        }
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
                File::open(path)?.write_all(self.export().as_bytes())
            }
        })*
    };
}

convenience!(Metadata, ArticleMetadata, CategoryMetadata);
