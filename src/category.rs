use futures::{Stream, stream::empty};

use crate::{
    article::Article,
    metadata::{CategoryMetadata, FailToOpenMetadata, MetadataExt},
    workspace::Workspace,
};

/// A category in the workspace
#[derive(Debug, Clone)]
pub struct Category {
    workspace: Workspace,
    segments: Vec<String>,
    metadata: CategoryMetadata,
}

impl Category {
    /// Create a new category with the given path and metadata
    #[must_use]
    pub const fn new(
        workspace: Workspace,
        segments: Vec<String>,
        metadata: CategoryMetadata,
    ) -> Self {
        Self {
            workspace,
            segments,
            metadata,
        }
    }

    #[must_use]
    pub const fn segments(&self) -> &Vec<String> {
        &self.segments
    }

    #[must_use]
    pub const fn metadata(&self) -> &CategoryMetadata {
        &self.metadata
    }
}

use std::{
    path::{Path, PathBuf},
    string::String,
    vec::Vec,
};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum FailToOpenCategory {
    #[error("Workspace not found")]
    WorkspaceNotFound,

    #[error("Category path is invalid")]
    InvalidPath,

    #[error("Unsupported path encoding")]
    UnsupportedPathEncoding,

    #[error("Failed to open category metadata: {0}")]
    FailToOpenMetadata(#[from] FailToOpenMetadata),
}

pub fn into_segments(path: &Path) -> Result<Vec<String>, FailToOpenCategory> {
    let segments: Vec<String> = path
        .components()
        .map(|c| {
            c.as_os_str()
                .to_str()
                .ok_or(FailToOpenCategory::UnsupportedPathEncoding)
                .map(|s| s.to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(segments)
}

impl Category {
    /// Open a category from the given root path and category path
    ///
    /// # Errors
    /// Returns `FailToOpenCategory::WorkspaceNotFound` if the category does not exist
    /// Returns `FailToOpenCategory::FailToOpenMetadata` if the metadata file cannot be opened
    pub async fn open(
        workspace: Workspace,
        path: impl AsRef<Path>,
    ) -> Result<Self, FailToOpenCategory> {
        let articles_dir = workspace.articles_dir();
        let path = path.as_ref();
        let segments: Vec<String> = path
            .strip_prefix(&articles_dir)
            .map_err(|_| FailToOpenCategory::InvalidPath)?
            .components()
            .map(|c| {
                c.as_os_str()
                    .to_str()
                    .map(|s| s.to_string())
                    .ok_or(FailToOpenCategory::UnsupportedPathEncoding)
            })
            .collect::<Result<Vec<_>, _>>()?;

        let metadata_path = path.join("Category.toml");

        let metadata = CategoryMetadata::open(metadata_path).await?;
        Ok(Self {
            segments,
            metadata,
            workspace,
        })
    }

    pub fn dir(&self) -> PathBuf {
        let articles_dir = self.workspace.articles_dir();
        self.segments
            .iter()
            .fold(articles_dir, |acc, comp| acc.join(comp))
    }

    // non-recursive listing of categories
    pub fn list_categories(&self) -> impl Stream<Item = Article> {
        empty() // TODO: implement category listing
    }

    // non-recursive listing of articles
    /// List the articles in this category
    ///
    /// # Errors
    /// Returns `FailToListArticles` if the articles directory cannot be read
    pub fn list_articles(&self) -> impl Stream<Item = Article> {
        empty() // TODO: implement article listing
    }
}

#[derive(Debug, Error)]
pub enum FailToListCategories {}

#[derive(Debug, Error)]
pub enum FailToListArticles {}
