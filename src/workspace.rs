use crate::{
    article::{Article, FailToOpenArticle},
    category::{Category, FailToOpenCategory},
    metadata::{FailToOpenMetadata, MetadataExt, PluginEntry, PluginRegistry, WorkspaceManifest},
};
use futures::stream::empty;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use thiserror::Error;
use tokio::fs::create_dir;
use tokio_stream::Stream;

/// structure of workspace is as follows:
/// ```text
/// /workspace-root
/// ├── .thought
/// ├── Thought.toml
/// ├── articles
/// │   ├── category1
/// │   │   ├── Article.toml
/// │   │   ├── article.md
/// │   │   ├── subcategory1
/// │   │   │   ├── Article.toml
/// │   │   │   ├── article.md
/// │   │   │   ├── image.png
/// │   ├── category2
/// │   │   ├── Article.toml
/// │   │   ├── article.md
/// │   ├── article.md
/// │   ├── Article.toml
/// ```
#[derive(Debug, Clone)]
pub struct Workspace(Arc<WorkspaceInner>);

#[derive(Debug, Clone)]
struct WorkspaceInner {
    path: PathBuf,
    manifest: WorkspaceManifest,
}

#[derive(Debug, Error)]
pub enum FailToCreateArticle {
    #[error("Invalid article path, must include at least a slug")]
    InvalidPath,
    #[error("Fail to open category: {0}")]
    Category(#[from] FailToOpenCategory),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

impl Workspace {
    pub async fn open(path: impl AsRef<std::path::Path>) -> Result<Self, FailToOpenMetadata> {
        let manifest_path = path.as_ref().join("Thought.toml");
        let manifest = WorkspaceManifest::open(manifest_path).await?;
        Ok(Self::new(path.as_ref(), manifest))
    }

    pub fn new(path: impl AsRef<std::path::Path>, manifest: WorkspaceManifest) -> Self {
        Self(
            WorkspaceInner {
                path: path.as_ref().to_path_buf(),
                manifest,
            }
            .into(),
        )
    }

    pub fn manifest_path(&self) -> PathBuf {
        self.root().join("Thought.toml")
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.0.path
    }

    pub fn articles_dir(&self) -> PathBuf {
        self.root().join("articles")
    }

    pub fn build_dir(&self) -> PathBuf {
        self.root().join("build")
    }

    pub fn cache_dir(&self) -> PathBuf {
        self.root().join(".thought")
    }

    pub async fn create(root: impl AsRef<Path>, name: String) -> color_eyre::eyre::Result<Self> {
        // create workspace directory

        let root = root.as_ref().join(&name);
        create_dir(&root).await?;

        let owner = detect_local_user();
        let mut registry = PluginRegistry::new();
        let theme = default_theme();
        registry.register_entry(theme);

        // create workspace manifest
        let manifest = WorkspaceManifest::new(name, "Thoughtful blog", owner, registry);
        manifest.save_to_file(root.join("Thought.toml")).await?;

        // create articles directory

        create_dir(root.join("articles")).await?;

        let workspace = Self::new(&root, manifest);

        let article = workspace.create_article(["hello"]).await?;

        Ok(workspace)
    }

    #[must_use]
    pub fn manifest(&self) -> &WorkspaceManifest {
        &self.0.manifest
    }

    pub async fn create_category(
        &self,
        path: impl Into<Vec<String>>,
        description: impl Into<String>,
    ) {
        todo!()
    }

    pub async fn save(&self) -> Result<(), std::io::Error> {
        let manifest_path = self.root().join("Thought.toml");
        self.0.manifest.save_to_file(manifest_path).await
    }

    pub async fn create_article(
        &self,
        path: impl IntoIterator<Item = impl AsRef<str>>,
    ) -> Result<Article, FailToCreateArticle> {
        todo!()
    }

    pub async fn generate(
        &self,
        output: impl AsRef<std::path::Path>,
    ) -> Result<(), std::io::Error> {
        todo!()
    }

    /// List all categories recursively in the workspace
    pub fn categories(
        &self,
    ) -> impl Stream<Item = Result<Category, FailToOpenCategory>> + Send + Sync + 'static {
        empty() // TODO: implement category listing
    }

    /// List all articles recursively in the workspace
    pub fn articles(&self) -> impl Stream<Item = Result<Article, FailToOpenArticle>> + Send + Sync {
        empty() // TODO: implement article listing
    }

    pub async fn clean(&self) -> Result<(), std::io::Error> {
        let build_dir = self.build_dir();
        if build_dir.exists() {
            tokio::fs::remove_dir_all(build_dir).await?;
        }

        let cache_dir = self.cache_dir();
        if cache_dir.exists() {
            tokio::fs::remove_dir_all(cache_dir).await?;
        }
        Ok(())
    }
}

fn detect_local_user() -> String {
    whoami::realname()
}

fn default_theme() -> PluginEntry {
    PluginEntry::git("zenflow", "https://github.com/lexoliu/zenflow.git", None)
}
