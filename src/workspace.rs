use crate::{
    article::{Article, FailToOpenArticle},
    category::{Category, FailToOpenCategory},
    engine::Engine,
    metadata::{
        ArticleMetadata, CategoryMetadata, FailToOpenMetadata, MetadataExt, PluginEntry,
        PluginRegistry, WorkspaceManifest,
    },
    utils::write,
};
use color_eyre::eyre::{self, eyre};
use futures::Stream;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use thiserror::Error;
use tokio::{
    fs::{self as async_fs, create_dir},
    sync::mpsc,
};
use tokio_stream::wrappers::UnboundedReceiverStream;

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

        let _ = workspace.create_article(["hello"]).await?;

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
    ) -> eyre::Result<()> {
        let segments: Vec<String> = path
            .into()
            .into_iter()
            .map(|segment| segment.trim().to_string())
            .filter(|segment| !segment.is_empty())
            .collect();

        if segments.is_empty() {
            return Err(eyre!("Category path cannot be empty"));
        }

        ensure_root_category(self).await?;

        let description = description.into();
        let mut current = self.articles_dir();

        for (index, segment) in segments.iter().enumerate() {
            current.push(segment);
            async_fs::create_dir_all(&current).await?;

            let desc = if index == segments.len() - 1 {
                Some(description.as_str())
            } else {
                None
            };
            ensure_category_metadata(&current, segment, desc).await?;
        }

        Ok(())
    }

    pub async fn save(&self) -> Result<(), std::io::Error> {
        let manifest_path = self.root().join("Thought.toml");
        self.0.manifest.save_to_file(manifest_path).await
    }

    pub async fn create_article(
        &self,
        path: impl IntoIterator<Item = impl AsRef<str>>,
    ) -> Result<Article, FailToCreateArticle> {
        let segments: Vec<String> = path
            .into_iter()
            .map(|segment| segment.as_ref().trim().to_string())
            .filter(|segment| !segment.is_empty())
            .collect();

        if segments.is_empty() {
            return Err(FailToCreateArticle::InvalidPath);
        }

        ensure_root_category(self)
            .await
            .map_err(FailToCreateArticle::Io)?;

        let slug = segments
            .last()
            .cloned()
            .ok_or(FailToCreateArticle::InvalidPath)?;

        let article_dir = segments
            .iter()
            .fold(self.articles_dir(), |acc, segment| acc.join(segment));
        async_fs::create_dir_all(&article_dir)
            .await
            .map_err(FailToCreateArticle::Io)?;

        let category_segments = if segments.len() > 1 {
            segments[..segments.len() - 1].to_vec()
        } else {
            Vec::new()
        };

        ensure_category_chain(self, &category_segments)
            .await
            .map_err(FailToCreateArticle::Io)?;

        let metadata_path = article_dir.join("Article.toml");
        if async_fs::metadata(&metadata_path).await.is_err() {
            let metadata = ArticleMetadata::new(self.manifest().owner().to_string());
            metadata
                .save_to_file(&metadata_path)
                .await
                .map_err(FailToCreateArticle::Io)?;
        }

        let content_path = article_dir.join("article.md");
        if async_fs::metadata(&content_path).await.is_err() {
            let template = format!("# {slug}\n\nWrite something thoughtful here.\n");
            write(&content_path, template.as_bytes())
                .await
                .map_err(FailToCreateArticle::Io)?;
        }

        Article::open(self.clone(), segments)
            .await
            .map_err(|err| match err {
                FailToOpenArticle::WorkspaceNotFound => {
                    FailToCreateArticle::Category(FailToOpenCategory::WorkspaceNotFound)
                }
                FailToOpenArticle::ArticleNotFound => FailToCreateArticle::InvalidPath,
                FailToOpenArticle::FailToOpenMetadata(inner) => {
                    FailToCreateArticle::Io(std::io::Error::other(inner))
                }
            })
    }

    pub async fn generate(&self, output: impl AsRef<std::path::Path>) -> eyre::Result<()> {
        let engine = Engine::new(self.clone()).await?;
        engine.generate(output).await
    }

    /// List all categories recursively in the workspace
    pub fn categories(
        &self,
    ) -> impl Stream<Item = Result<Category, FailToOpenCategory>> + Send + Sync + 'static {
        let (tx, rx) = mpsc::unbounded_channel();
        let workspace = self.clone();
        let root = workspace.articles_dir();
        tokio::spawn(async move {
            if let Err(err) = walk_categories(workspace, root, tx.clone()).await {
                let _ = tx.send(Err(err));
            }
        });
        UnboundedReceiverStream::new(rx)
    }

    /// List all articles recursively in the workspace
    pub fn articles(&self) -> impl Stream<Item = Result<Article, FailToOpenArticle>> + Send + Sync {
        let (tx, rx) = mpsc::unbounded_channel();
        let workspace = self.clone();
        let root = workspace.articles_dir();
        tokio::spawn(async move {
            if let Err(err) = walk_articles(workspace, root.clone(), root, tx.clone()).await {
                let _ = tx.send(Err(err));
            }
        });
        UnboundedReceiverStream::new(rx)
    }

    pub async fn read_article(&self, path: impl AsRef<Path>) -> Result<Article, FailToOpenArticle> {
        let relative = path
            .as_ref()
            .strip_prefix(self.articles_dir())
            .map_err(|_| FailToOpenArticle::ArticleNotFound)?;
        let segments = relative
            .components()
            .map(|component| {
                component
                    .as_os_str()
                    .to_str()
                    .map(|segment| segment.to_string())
                    .ok_or(FailToOpenArticle::ArticleNotFound)
            })
            .collect::<Result<Vec<_>, _>>()?;
        Article::open(self.clone(), segments).await
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

async fn ensure_category_metadata(
    dir: &Path,
    name: &str,
    description: Option<&str>,
) -> std::io::Result<()> {
    let metadata_path = dir.join("Category.toml");
    if async_fs::metadata(&metadata_path).await.is_err() {
        let mut metadata = CategoryMetadata::new(name);
        if let Some(desc) = description {
            metadata.set_description(desc);
        }
        metadata.save_to_file(&metadata_path).await?;
        return Ok(());
    }

    if let Some(desc) = description {
        if desc.is_empty() {
            return Ok(());
        }
        let mut metadata = CategoryMetadata::open(&metadata_path)
            .await
            .map_err(std::io::Error::other)?;
        metadata.set_description(desc);
        metadata.save_to_file(&metadata_path).await?;
    }
    Ok(())
}

async fn ensure_root_category(workspace: &Workspace) -> std::io::Result<()> {
    async_fs::create_dir_all(workspace.articles_dir()).await?;
    ensure_category_metadata(
        &workspace.articles_dir(),
        workspace.manifest().name(),
        Some(workspace.manifest().description()),
    )
    .await
}

async fn ensure_category_chain(workspace: &Workspace, segments: &[String]) -> std::io::Result<()> {
    if segments.is_empty() {
        return ensure_root_category(workspace).await;
    }

    let mut current = workspace.articles_dir();
    for segment in segments {
        current.push(segment);
        async_fs::create_dir_all(&current).await?;
        ensure_category_metadata(&current, segment, None).await?;
    }
    Ok(())
}

async fn walk_categories(
    workspace: Workspace,
    start: PathBuf,
    tx: mpsc::UnboundedSender<Result<Category, FailToOpenCategory>>,
) -> Result<(), FailToOpenCategory> {
    let mut stack = vec![start];
    while let Some(dir) = stack.pop() {
        let mut entries = async_fs::read_dir(&dir)
            .await
            .map_err(|_| FailToOpenCategory::WorkspaceNotFound)?;
        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|_| FailToOpenCategory::WorkspaceNotFound)?
        {
            let path = entry.path();
            if entry
                .file_type()
                .await
                .map_err(|_| FailToOpenCategory::WorkspaceNotFound)?
                .is_dir()
            {
                if async_fs::metadata(path.join("Category.toml")).await.is_ok() {
                    match Category::open(workspace.clone(), &path).await {
                        Ok(category) => {
                            if tx.send(Ok(category)).is_err() {
                                return Ok(());
                            }
                        }
                        Err(err) => {
                            if tx.send(Err(err)).is_err() {
                                return Ok(());
                            }
                        }
                    }
                }
                stack.push(path);
            }
        }
    }
    Ok(())
}

async fn walk_articles(
    workspace: Workspace,
    root: PathBuf,
    start: PathBuf,
    tx: mpsc::UnboundedSender<Result<Article, FailToOpenArticle>>,
) -> Result<(), FailToOpenArticle> {
    let mut stack = vec![start];
    while let Some(dir) = stack.pop() {
        let mut entries = async_fs::read_dir(&dir)
            .await
            .map_err(|_| FailToOpenArticle::WorkspaceNotFound)?;
        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|_| FailToOpenArticle::WorkspaceNotFound)?
        {
            let path = entry.path();
            if entry
                .file_type()
                .await
                .map_err(|_| FailToOpenArticle::WorkspaceNotFound)?
                .is_dir()
            {
                if async_fs::metadata(path.join("Article.toml")).await.is_ok() {
                    let relative = path
                        .strip_prefix(&root)
                        .map_err(|_| FailToOpenArticle::WorkspaceNotFound)?;
                    let segments = relative
                        .components()
                        .map(|component| {
                            component
                                .as_os_str()
                                .to_str()
                                .map(|segment| segment.to_string())
                                .ok_or(FailToOpenArticle::WorkspaceNotFound)
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    match Article::open(workspace.clone(), segments).await {
                        Ok(article) => {
                            if tx.send(Ok(article)).is_err() {
                                return Ok(());
                            }
                        }
                        Err(err) => {
                            if tx.send(Err(err)).is_err() {
                                return Ok(());
                            }
                        }
                    }
                    continue;
                }
                stack.push(path);
            }
        }
    }
    Ok(())
}
