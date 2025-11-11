use futures::Stream;
use tokio::{fs, sync::mpsc};
use tokio_stream::wrappers::UnboundedReceiverStream;

use crate::{
    article::{Article, FailToOpenArticle},
    metadata::{CategoryMetadata, FailToOpenMetadata, MetadataExt},
    workspace::Workspace,
};

/// A category in the workspace
#[derive(Debug, Clone)]
pub struct Category {
    workspace: Workspace,
    pub(crate) segments: Vec<String>,
    pub(crate) metadata: CategoryMetadata,
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

    #[must_use]
    pub fn workspace(&self) -> Workspace {
        self.workspace.clone()
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

    /// List immediate child categories.
    pub fn list_categories(
        &self,
    ) -> impl Stream<Item = Result<Category, FailToListCategories>> + Send + Sync + 'static {
        let (tx, rx) = mpsc::unbounded_channel();
        let category = self.clone();
        tokio::spawn(async move {
            if let Err(err) = list_child_categories(category, tx.clone()).await {
                let _ = tx.send(Err(err));
            }
        });
        UnboundedReceiverStream::new(rx)
    }

    // non-recursive listing of articles
    /// List the articles in this category
    ///
    /// # Errors
    /// Returns `FailToListArticles` if the articles directory cannot be read
    pub fn list_articles(
        &self,
    ) -> impl Stream<Item = Result<Article, FailToListArticles>> + Send + Sync + 'static {
        let (tx, rx) = mpsc::unbounded_channel();
        let category = self.clone();
        tokio::spawn(async move {
            if let Err(err) = list_category_articles(category, tx.clone()).await {
                let _ = tx.send(Err(err));
            }
        });
        UnboundedReceiverStream::new(rx)
    }
}

#[derive(Debug, Error)]
pub enum FailToListCategories {
    #[error("I/O error while listing categories: {0}")]
    Io(#[from] std::io::Error),
    #[error("Failed to open category: {0}")]
    Category(#[from] FailToOpenCategory),
}

#[derive(Debug, Error)]
pub enum FailToListArticles {
    #[error("I/O error while listing articles: {0}")]
    Io(#[from] std::io::Error),
    #[error("Failed to open article: {0}")]
    Article(#[from] FailToOpenArticle),
}

async fn list_child_categories(
    category: Category,
    tx: mpsc::UnboundedSender<Result<Category, FailToListCategories>>,
) -> Result<(), FailToListCategories> {
    let mut entries = fs::read_dir(category.dir()).await?;
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        if entry.file_type().await?.is_dir()
            && fs::metadata(path.join("Category.toml")).await.is_ok()
        {
            match Category::open(category.workspace(), &path).await {
                Ok(child) => {
                    if tx.send(Ok(child)).is_err() {
                        break;
                    }
                }
                Err(err) => {
                    if tx.send(Err(FailToListCategories::Category(err))).is_err() {
                        break;
                    }
                }
            }
        }
    }
    Ok(())
}

async fn list_category_articles(
    category: Category,
    tx: mpsc::UnboundedSender<Result<Article, FailToListArticles>>,
) -> Result<(), FailToListArticles> {
    let mut entries = fs::read_dir(category.dir()).await?;
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        if entry.file_type().await?.is_dir()
            && fs::metadata(path.join("Article.toml")).await.is_ok()
        {
            let relative = path
                .strip_prefix(category.workspace().articles_dir())
                .map_err(std::io::Error::other)?;
            let segments = relative
                .components()
                .map(|component| {
                    component
                        .as_os_str()
                        .to_str()
                        .map(|segment| segment.to_string())
                        .ok_or_else(|| {
                            std::io::Error::new(
                                std::io::ErrorKind::InvalidData,
                                "Invalid UTF-8 in article path",
                            )
                        })
                })
                .collect::<Result<Vec<_>, _>>()?;

            match Article::open(category.workspace(), segments).await {
                Ok(article) => {
                    if tx.send(Ok(article)).is_err() {
                        break;
                    }
                }
                Err(err) => {
                    if tx.send(Err(FailToListArticles::Article(err))).is_err() {
                        break;
                    }
                }
            }
        }
    }
    Ok(())
}
