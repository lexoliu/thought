use std::path::PathBuf;

use smol::stream::{Stream, once};
use thought_core::{
    article::Article,
    metadata::{FailToOpenMetadata, MetadataExt, ThemeSource, WorkspaceMetadata},
};

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

pub enum FailToCreateCategory {
    Io(std::io::Error),
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

    pub async fn create(
        root: impl AsRef<std::path::Path>,
        title: String,
        description: String,
    ) -> Result<Self, std::io::Error> {
        let metadata_path = root.as_ref().join("Thought.toml");
        let owner = detect_local_user();
        let theme = default_theme();
        let metadata = WorkspaceMetadata::new(title, description, owner, theme);
        metadata.save_to_file(&metadata_path).await?;
        Ok(Self {
            path: root.as_ref().to_path_buf(),
            metadata,
        })
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
        todo!()
    }

    pub async fn create_article(
        &self,
        path: impl Into<Vec<String>>,
    ) -> Result<thought_core::article::Article, FailToCreateCategory> {
        todo!()
    }

    pub async fn generate(
        &self,
        output: impl AsRef<std::path::Path>,
    ) -> Result<(), std::io::Error> {
        todo!()
    }

    pub fn categories(&self) -> impl Stream<Item = Result<Self, FailToOpenMetadata>> + Send + Sync {
        once(todo!())
    }

    pub fn articles(&self) -> impl Stream<Item = Article> + Send + Sync {
        once(todo!())
    }

    pub async fn clean(&self) -> Result<(), std::io::Error> {
        let build_path = self.path.join("build");
        if build_path.exists() {
            smol::fs::remove_dir_all(build_path).await?;
        }

        let cache_path = self.path.join(".cache");
        if cache_path.exists() {
            smol::fs::remove_dir_all(cache_path).await?;
        }
        Ok(())
    }
}

fn detect_local_user() -> String {
    whoami::realname()
}

fn default_theme() -> ThemeSource {
    ThemeSource::git("zenflow", "https://github.com/lexoliu/zenflow.git", None)
}
