//! Metadata structures and utilities
//! This module provides the data structures and traits for working with article and category metadata.

use std::{collections::BTreeMap, path::PathBuf};

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use time::OffsetDateTime;

use crate::utils::{read_to_string, write};

/// Metadata for a category
///
/// It always locates in a `Category.toml` file inside the category directory.
/// ```plain
/// /articles
///  /programming
///     Category.toml  <--- Category metadata file
///
/// ```
#[derive(Debug, Clone, Deserialize, Serialize, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct CategoryMetadata {
    #[serde(with = "time::serde::rfc3339")]
    created: OffsetDateTime,
    name: String,
    #[serde(default)]
    description: String,
}

/// Metadata for an article
/// It always locates in an `Article.toml` file inside the article directory.
/// ```plain
/// /articles
///  /programming
///     /my-first-article
///        Article.toml  <--- Article metadata file
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct ArticleMetadata {
    #[serde(with = "time::serde::rfc3339")]
    created: OffsetDateTime,
    #[serde(default)]
    tags: Vec<String>,
    author: String,
    description: Option<String>,
}

impl ArticleMetadata {
    /// Create a new article metadata with the given author
    pub fn new(author: impl Into<String>) -> Self {
        Self {
            created: OffsetDateTime::now_utc(),
            author: author.into(),
            tags: Vec::new(),
            description: None,
        }
    }

    /// Get the description of the article
    #[must_use]
    pub const fn description(&self) -> Option<&str> {
        if let Some(desc) = &self.description {
            Some(desc.as_str())
        } else {
            None
        }
    }

    /// Set the description of the article
    pub fn set_description(&mut self, description: impl Into<String>) {
        self.description = Some(description.into());
    }

    /// Create a new article metadata file at the given path with the given author
    #[must_use]
    pub const fn created(&self) -> OffsetDateTime {
        self.created
    }

    /// Get the author of the article
    #[must_use]
    pub fn author(&self) -> &str {
        &self.author
    }

    /// Set the author of the article
    pub fn set_author(&mut self, author: impl Into<String>) {
        self.author = author.into();
    }

    /// Get the tags of the article
    #[must_use]
    pub fn tags(&self) -> &[String] {
        &self.tags
    }

    /// Add a tag to the article
    pub fn add_tag(&mut self, tag: impl Into<String>) {
        self.tags.push(tag.into());
    }
}

impl CategoryMetadata {
    /// Create a new category metadata with the given name
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            created: OffsetDateTime::now_utc(),
            name: name.into(),
            description: String::new(),
        }
    }

    /// Get the creation time of the category
    #[must_use]
    pub const fn created(&self) -> OffsetDateTime {
        self.created
    }

    /// Get the name of the category
    #[must_use]
    pub fn name(&self) -> &str {
        self.name.as_str()
    }

    /// Get the description of the category
    #[must_use]
    pub fn description(&self) -> &str {
        self.description.as_str()
    }
}

/// Metadata for a workspace (your entire blog)
/// Locate in `Thought.toml` at the root of the workspace
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceMetadata {
    title: String,
    description: String,
    owner: String,
    theme: ThemeSource,
    plugins: BTreeMap<String, PluginSource>,
}

impl WorkspaceMetadata {
    /// Create a new workspace metadata with the given parameters
    pub fn new(
        title: impl Into<String>,
        description: impl Into<String>,
        owner: impl Into<String>,
        theme: ThemeSource,
    ) -> Self {
        Self {
            title: title.into(),
            description: description.into(),
            owner: owner.into(),
            theme,
            plugins: BTreeMap::new(),
        }
    }

    /// Set the owner of the workspace
    pub fn set_owner(&mut self, owner: impl Into<String>) {
        self.owner = owner.into();
    }

    /// Get the title of the workspace
    #[must_use]
    pub const fn title(&self) -> &str {
        self.title.as_str()
    }

    /// Get the description of the workspace
    #[must_use]
    pub const fn description(&self) -> &str {
        self.description.as_str()
    }

    /// Get the owner of the workspace
    #[must_use]
    pub const fn owner(&self) -> &str {
        self.owner.as_str()
    }

    /// Get the theme source of the workspace
    #[must_use]
    pub const fn theme(&self) -> &ThemeSource {
        &self.theme
    }

    /// Get the plugins of the workspace
    #[must_use]
    pub const fn plugins(&self) -> &BTreeMap<String, PluginSource> {
        &self.plugins
    }
}

/// Source of a theme
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThemeSource {
    name: String,
    #[serde(flatten)]
    source: PluginSource,
}

impl ThemeSource {
    /// Create a new theme source with the given name and source
    pub fn new(name: impl Into<String>, source: PluginSource) -> Self {
        Self {
            name: name.into(),
            source,
        }
    }

    /// Create a new theme source from a Git repository
    pub fn git(name: impl Into<String>, repo: impl Into<String>, rev: Option<String>) -> Self {
        Self {
            name: name.into(),
            source: PluginSource::Git {
                repo: repo.into(),
                rev,
            },
        }
    }

    /// Get the name of the theme
    #[must_use]
    pub const fn name(&self) -> &str {
        self.name.as_str()
    }

    /// Get the source of the theme
    #[must_use]
    pub const fn source(&self) -> &PluginSource {
        &self.source
    }
}

/// Source of a plugin
///
/// Plugins can be sourced from different locations, such as crates.io, Git repositories, local paths, or URLs.
///
/// `Thought` would load plugins to its wasm runtime from these sources accordingly.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PluginSource {
    /// Plugin from crates.io with the given version
    CratesIo {
        /// Version requirement (e.g., "0.1", "^1.2.3")
        version: String,
    },
    /// Plugin from a Git repository
    Git {
        /// Git repository URL
        repo: String,
        /// Git revision (branch, tag, or commit)
        rev: Option<String>,
    },
    /// Plugin from local filesystem
    Local(PathBuf),
}

/// Errors that can occur when opening metadata files
#[derive(Debug, thiserror::Error)]
pub enum FailToOpenMetadata {
    /// I/O error
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    /// TOML parse error
    #[error("TOML parse error: {0}")]
    TomlParse(#[from] toml::de::Error),
}

/// Extension trait for metadata serialization and file operations
pub trait MetadataExt: Serialize + DeserializeOwned {
    /// Export the metadata to a TOML string
    ///
    /// # Errors
    /// Returns an `std::io::Error` if the file cannot be read or parsed
    fn open(
        path: impl AsRef<std::path::Path>,
    ) -> impl Future<Output = Result<Self, FailToOpenMetadata>> + Send + Sync {
        let path = path.as_ref().to_path_buf();
        async move {
            let content = read_to_string(&path).await?;
            let metadata = toml::from_str(&content)?;
            Ok(metadata)
        }
    }
    /// Export the metadata to a TOML string
    #[must_use]
    fn to_toml(&self) -> String {
        toml::to_string_pretty(self).expect("Failed to serialize metadata to TOML")
    }

    /// Save the metadata to a file at the given path
    /// # Errors
    /// Returns an `std::io::Error` if the file cannot be written
    fn save_to_file(
        &self,
        path: impl AsRef<std::path::Path>,
    ) -> impl Future<Output = Result<(), std::io::Error>> + Send + Sync {
        let path = path.as_ref().to_path_buf();
        let toml_str = self.to_toml();
        async move { write(path, toml_str.as_bytes()).await }
    }
}

impl MetadataExt for CategoryMetadata {}
impl MetadataExt for ArticleMetadata {}
impl MetadataExt for WorkspaceMetadata {}
