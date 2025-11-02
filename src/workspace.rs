use std::path::PathBuf;

use smol::stream::{once, Stream};
use thought_core::{
    article::Article,
    metadata::{FailToOpenMetadata, MetadataExt, WorkspaceMetadata},
};

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

    pub fn articles(&self) -> impl Stream<Item = Article> + Send + Sync {
        once(todo!())
    }
}
