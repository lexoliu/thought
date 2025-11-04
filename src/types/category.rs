use serde::{Deserialize, Serialize};

use crate::types::metadata::CategoryMetadata;

/// A category in the workspace
#[derive(Debug, Clone, Serialize, Deserialize, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct Category {
    path: Vec<String>,
    metadata: CategoryMetadata,
}

impl Category {
    /// Create a new category with the given path and metadata
    #[must_use]
    pub const fn new(path: Vec<String>, metadata: CategoryMetadata) -> Self {
        Self { path, metadata }
    }

    #[must_use]
    pub const fn path(&self) -> &Vec<String> {
        &self.path
    }

    #[must_use]
    pub const fn metadata(&self) -> &CategoryMetadata {
        &self.metadata
    }
}

use std::{path::Path, string::String, vec::Vec};

use thiserror::Error;

use crate::types::metadata::{FailToOpenMetadata, MetadataExt};

#[derive(Debug, Error)]
pub enum FailToOpenCategory {
    #[error("Workspace not found")]
    WorkspaceNotFound,

    #[error("Failed to open category metadata: {0}")]
    FailToOpenMetadata(#[from] FailToOpenMetadata),
}

impl Category {
    /// Open a category from the given root path and category path
    ///
    /// # Errors
    /// Returns `FailToOpenCategory::WorkspaceNotFound` if the category does not exist
    /// Returns `FailToOpenCategory::FailToOpenMetadata` if the metadata file cannot be opened
    pub async fn open(
        root: impl AsRef<Path>,
        path: Vec<String>,
    ) -> Result<Self, FailToOpenCategory> {
        let path_buf = root.as_ref().join("articles");
        let full_path = path.iter().fold(path_buf, |acc, comp| acc.join(comp));
        let metadata_path = full_path.join("Category.toml");

        // check if the category directory exists
        if !full_path.exists() || !full_path.is_dir() {
            return Err(FailToOpenCategory::WorkspaceNotFound);
        }

        let metadata = CategoryMetadata::open(metadata_path).await?;
        Ok(Self { path, metadata })
    }
}
