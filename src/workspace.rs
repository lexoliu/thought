use crate::{
    engine::Engine,
    types::{
        article::Article,
        category::{Category, FailToOpenCategory},
        metadata::{
            ArticleMetadata, CategoryMetadata, FailToOpenMetadata, MetadataExt, ThemeSource,
            WorkspaceMetadata,
        },
    },
    utils::write,
};
use futures::StreamExt;
use std::{
    fs as std_fs, io,
    path::{Path, PathBuf},
};
use thiserror::Error;
use time::OffsetDateTime;
use tokio::fs::create_dir;
use tokio_stream::Stream;
use tracing::warn;

/// structure of workspace is as follows:
/// ```text
/// /workspace-root
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
#[derive(Clone)]
pub struct Workspace {
    path: PathBuf,
    metadata: WorkspaceMetadata,
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
        let metadata_path = path.as_ref().join("Thought.toml");
        let metadata = WorkspaceMetadata::open(metadata_path).await?;
        Ok(Self {
            path: path.as_ref().to_path_buf(),
            metadata,
        })
    }

    #[must_use]
    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    pub async fn create(root: impl AsRef<Path>, name: String) -> color_eyre::eyre::Result<Self> {
        // create workspace directory

        let root = root.as_ref().join(&name);
        create_dir(&root).await?;
        let owner = detect_local_user();
        let theme = default_theme();
        // create workspace metadata
        let metadata = WorkspaceMetadata::new(name, "Thoughtful blog", owner, theme);
        metadata.save_to_file(root.join("Thought.toml")).await?;

        // create articles directory

        create_dir(root.join("articles")).await?;

        let workspace = Self {
            path: root.to_path_buf(),
            metadata,
        };

        let article = workspace.create_article(["hello"]).await?;

        Ok(workspace)
    }

    pub fn set_owner(&mut self, owner: String) {
        self.metadata.set_owner(owner);
    }

    #[must_use]
    pub const fn metadata(&self) -> &WorkspaceMetadata {
        &self.metadata
    }

    pub async fn create_category(
        &self,
        path: impl Into<Vec<String>>,
        description: impl Into<String>,
    ) {
        let path_vec = path.into();
        let description = description.into();
        if let Err(err) = self
            .ensure_category_chain(&path_vec, Some(description.as_str()))
            .await
        {
            warn!(
                "failed to create category `{}`: {err}",
                format_category_path(&path_vec)
            );
        }
    }

    pub async fn create_article(
        &self,
        path: impl IntoIterator<Item = impl AsRef<str>>,
    ) -> Result<Article, FailToCreateArticle> {
        let mut segments = path
            .into_iter()
            .map(|s| s.as_ref().to_string())
            .collect::<Vec<String>>();
        if segments.is_empty() {
            return Err(FailToCreateArticle::InvalidPath);
        }

        let slug = segments
            .pop()
            .expect("article path guaranteed to contain a slug");
        let category_path = segments;

        self.ensure_category_chain(&category_path, None).await?;

        let mut article_dir = self.path.join("articles");
        for segment in &category_path {
            article_dir.push(segment);
        }
        article_dir.push(&slug);
        tokio::fs::create_dir_all(&article_dir).await?;

        let metadata_path = article_dir.join("Article.toml");
        let content_path = article_dir.join("article.md");

        let title = slug_to_title(&slug);
        let mut metadata = ArticleMetadata::new(self.metadata.owner().to_owned());
        let summary = format!("Draft article for {title}");
        metadata.set_description(summary.clone());
        metadata.save_to_file(&metadata_path).await?;

        let content = format!("# {title}\n\nWrite your article here.\n");
        write(&content_path, content.as_bytes()).await?;

        let category = Category::open(&self.path, category_path.clone()).await?;

        Ok(Article::new(
            title, slug, category, metadata, summary, content,
        ))
    }

    pub async fn generate(
        &self,
        output: impl AsRef<std::path::Path>,
    ) -> Result<(), std::io::Error> {
        let engine = Engine::new(self.clone())
            .await
            .map_err(|err| io::Error::other(err.to_string()))?;
        engine.generate(output).await
    }

    pub fn categories(
        &self,
    ) -> impl Stream<Item = Result<Category, FailToOpenMetadata>> + Send + Sync {
        let root = self.path.clone();
        let paths = match collect_category_paths(&root.join("articles")) {
            Ok(paths) => paths,
            Err(err) => {
                warn!("failed to enumerate categories: {err}");
                Vec::new()
            }
        };

        tokio_stream::iter(paths).then(move |path| {
            let root = root.clone();
            async move {
                let metadata_path = path
                    .iter()
                    .fold(root.join("articles"), |acc, segment| acc.join(segment))
                    .join("Category.toml");
                let metadata = CategoryMetadata::open(metadata_path).await?;
                Ok(Category::new(path, metadata))
            }
        })
    }

    pub fn articles(&self) -> impl Stream<Item = Article> + Send + Sync {
        let root = self.path.clone();
        let paths = match collect_article_paths(&root.join("articles")) {
            Ok(paths) => paths,
            Err(err) => {
                warn!("failed to enumerate articles: {err}");
                Vec::new()
            }
        };

        tokio_stream::iter(paths)
            .then(move |path| {
                let root = root.clone();
                async move { Article::open(&root, path).await.ok() }
            })
            .filter_map(|article| async move { article })
    }

    async fn ensure_category_chain(
        &self,
        path: &[String],
        description: Option<&str>,
    ) -> io::Result<()> {
        let articles_root = self.path.join("articles");
        tokio::fs::create_dir_all(&articles_root).await?;

        let root_metadata_path = articles_root.join("Category.toml");
        if !tokio::fs::try_exists(&root_metadata_path).await? {
            let metadata = CategoryMetadata::from_raw(
                OffsetDateTime::now_utc(),
                self.metadata.name().to_owned(),
                self.metadata.description().to_owned(),
            );
            metadata.save_to_file(&root_metadata_path).await?;
        } else if let Some(desc) = description
            && path.is_empty()
        {
            let existing = CategoryMetadata::open(&root_metadata_path)
                .await
                .map_err(|err| io::Error::other(err.to_string()))?;
            let updated = CategoryMetadata::from_raw(
                existing.created(),
                existing.name().to_owned(),
                desc.to_owned(),
            );
            updated.save_to_file(&root_metadata_path).await?;
        }

        let mut current = articles_root;
        for (index, segment) in path.iter().enumerate() {
            current.push(segment);
            tokio::fs::create_dir_all(&current).await?;
            let metadata_path = current.join("Category.toml");
            if tokio::fs::try_exists(&metadata_path).await? {
                if index == path.len() - 1
                    && let Some(desc) = description
                {
                    let existing = CategoryMetadata::open(&metadata_path)
                        .await
                        .map_err(|err| io::Error::other(err.to_string()))?;
                    let updated = CategoryMetadata::from_raw(
                        existing.created(),
                        existing.name().to_owned(),
                        desc.to_owned(),
                    );
                    updated.save_to_file(&metadata_path).await?;
                }
            } else {
                let desc = if index == path.len() - 1 {
                    description.unwrap_or_default()
                } else {
                    ""
                };
                let metadata = CategoryMetadata::from_raw(
                    OffsetDateTime::now_utc(),
                    segment.clone(),
                    desc.to_owned(),
                );
                metadata.save_to_file(&metadata_path).await?;
            }
        }

        Ok(())
    }

    pub async fn clean(&self) -> Result<(), std::io::Error> {
        let build_path = self.path.join("build");
        if build_path.exists() {
            tokio::fs::remove_dir_all(build_path).await?;
        }

        let cache_path = self.path.join(".cache");
        if cache_path.exists() {
            tokio::fs::remove_dir_all(cache_path).await?;
        }
        Ok(())
    }
}

fn collect_category_paths(root: &Path) -> io::Result<Vec<Vec<String>>> {
    let mut categories = Vec::new();
    if !root.exists() {
        return Ok(categories);
    }

    fn walk(dir: &Path, prefix: &mut Vec<String>, acc: &mut Vec<Vec<String>>) -> io::Result<()> {
        if dir.join("Category.toml").exists() {
            acc.push(prefix.clone());
        }

        for entry in std_fs::read_dir(dir)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                let entry_path = entry.path();
                let Ok(name) = entry.file_name().into_string() else {
                    continue;
                };
                prefix.push(name);
                walk(&entry_path, prefix, acc)?;
                prefix.pop();
            }
        }

        Ok(())
    }

    let mut prefix = Vec::new();
    walk(root, &mut prefix, &mut categories)?;
    Ok(categories)
}

fn collect_article_paths(root: &Path) -> io::Result<Vec<Vec<String>>> {
    let mut articles = Vec::new();
    if !root.exists() {
        return Ok(articles);
    }

    fn walk(dir: &Path, prefix: &mut Vec<String>, acc: &mut Vec<Vec<String>>) -> io::Result<()> {
        for entry in std_fs::read_dir(dir)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                let entry_path = entry.path();
                let Ok(name) = entry.file_name().into_string() else {
                    continue;
                };
                prefix.push(name);
                if entry_path.join("Article.toml").exists() {
                    acc.push(prefix.clone());
                    prefix.pop();
                    continue;
                }
                walk(&entry_path, prefix, acc)?;
                prefix.pop();
            }
        }
        Ok(())
    }

    let mut prefix = Vec::new();
    walk(root, &mut prefix, &mut articles)?;
    Ok(articles)
}

fn slug_to_title(slug: &str) -> String {
    let mut words = Vec::new();

    for word in slug.split(['-', '_', ' ']) {
        if word.is_empty() {
            continue;
        }

        let mut chars = word.chars();
        if let Some(first) = chars.next() {
            let mut capitalized = String::new();
            capitalized.extend(first.to_uppercase());
            capitalized.extend(chars);
            words.push(capitalized);
        }
    }

    if words.is_empty() {
        "Untitled Article".to_string()
    } else {
        words.join(" ")
    }
}

fn format_category_path(path: &[String]) -> String {
    if path.is_empty() {
        "<root>".to_string()
    } else {
        path.join("/")
    }
}

fn detect_local_user() -> String {
    whoami::realname()
}

fn default_theme() -> ThemeSource {
    ThemeSource::git("zenflow", "https://github.com/lexoliu/zenflow.git", None)
}
