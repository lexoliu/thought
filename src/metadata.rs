//! Metadata structures and utilities
//! This module provides the data structures and traits for working with article and category metadata.

use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    str::FromStr,
};

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

    /// Update the description of the category
    pub fn set_description(&mut self, description: impl Into<String>) {
        self.description = description.into();
    }
}

/// Metadata for a workspace (your entire blog)
/// Locate in `Thought.toml` at the root of the workspace
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceManifest {
    name: String,
    description: String,
    owner: String,
    plugins: PluginRegistry,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginRegistry {
    map: HashMap<String, PluginLocator>,
}

impl Default for PluginRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl PluginRegistry {
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
        }
    }

    pub fn register(&mut self, name: impl Into<String>, locator: PluginLocator) {
        self.map.insert(name.into(), locator);
    }

    pub fn register_entry(&mut self, entry: PluginEntry) {
        self.map.insert(entry.name, entry.locator);
    }

    /// Get an iterator over the registered plugins
    pub fn plugins(&self) -> impl Iterator<Item = (&str, &PluginLocator)> + Send + Sync {
        self.map.iter().map(|(k, v)| (k.as_str(), v))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginEntry {
    name: String,
    #[serde(flatten)]
    locator: PluginLocator,
}

impl PluginEntry {
    #[must_use]
    pub fn name(&self) -> &str {
        self.name.as_str()
    }

    #[must_use]
    pub fn locator(&self) -> &PluginLocator {
        &self.locator
    }

    pub fn git(
        name: impl Into<String>,
        url: impl Into<String>,
        rev: impl Into<Option<String>>,
    ) -> Self {
        Self {
            name: name.into(),
            locator: PluginLocator::Git {
                url: url.into(),
                rev: rev.into(),
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
// not tag
#[serde(untagged)]
pub enum PluginLocator {
    CratesIo { version: String },
    Git { url: String, rev: Option<String> },
    Local { path: PathBuf },
}

impl WorkspaceManifest {
    /// Create a new workspace metadata with the given parameters
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        owner: impl Into<String>,
        plugins: PluginRegistry,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            owner: owner.into(),
            plugins,
        }
    }

    /// Set the owner of the workspace
    pub fn set_owner(&mut self, owner: impl Into<String>) {
        self.owner = owner.into();
    }

    /// Get the name of the workspace
    #[must_use]
    pub const fn name(&self) -> &str {
        self.name.as_str()
    }

    /// Get the description of the workspace
    #[must_use]
    pub const fn description(&self) -> &str {
        self.description.as_str()
    }

    // return a iterator over plugins
    pub fn plugins(&self) -> impl Iterator<Item = (&str, &PluginLocator)> + Send + Sync {
        self.plugins.plugins()
    }

    /// Get the owner of the workspace
    #[must_use]
    pub const fn owner(&self) -> &str {
        self.owner.as_str()
    }
}

/// Classification of plugin roles.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PluginKind {
    Theme,
    Hook,
}

/// Errors that can occur while loading a plugin manifest.
#[derive(Debug, thiserror::Error)]
pub enum ManifestError {
    /// Failed to read the manifest file.
    #[error("failed to read Plugin.toml: {0}")]
    Io(#[from] std::io::Error),
    /// Failed to parse the manifest TOML.
    #[error("failed to parse Plugin.toml: {0}")]
    Parse(#[from] toml::de::Error),
    /// A required field is missing.
    #[error("missing required field `{0}` in Plugin.toml")]
    MissingField(&'static str),
}

/// Metadata declared by an individual plugin.
/// Locate in `Plugin.toml` at the root of the plugin package.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PluginManifest {
    pub name: String,
    pub author: String,
    pub version: String,
    #[serde(rename = "type")]
    pub kind: PluginKind,
    #[serde(default)]
    pub description: Option<String>,
}

impl PluginManifest {
    /// Load a `Plugin.toml` from disk.
    ///
    /// # Errors
    /// Returns [`ManifestError`] for missing files, malformed TOML, or absent required fields.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, ManifestError> {
        let data = fs::read_to_string(path)?;
        let manifest: Self = toml::from_str(&data)?;
        if manifest.name.trim().is_empty() {
            return Err(ManifestError::MissingField("name"));
        }
        if manifest.author.trim().is_empty() {
            return Err(ManifestError::MissingField("author"));
        }
        if manifest.version.trim().is_empty() {
            return Err(ManifestError::MissingField("version"));
        }
        Ok(manifest)
    }
}

impl FromStr for PluginKind {
    type Err = ManifestError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "theme" => Ok(Self::Theme),
            "hook" => Ok(Self::Hook),
            _ => Err(ManifestError::MissingField("type")),
        }
    }
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
impl MetadataExt for WorkspaceManifest {}
impl MetadataExt for PluginManifest {}
