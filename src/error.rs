use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;
#[derive(Debug, Error)]
pub enum Error {
    #[error("Inner error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Cannot parse template: {0}")]
    Templte(#[from] liquid::Error),
    #[error("Workspace already exists")]
    WorkspaceAlreadyExists,
    #[error("Article not found")]
    ArticleNotFound,
    #[error("Workspace not found")]
    WorkspaceNotFound,
    #[error("Post already exists")]
    PostAlreadyExists,
    #[error("Invalid config: {0}")]
    InvalidConfig(toml::de::Error),
    #[error("Invalid metadata: {0}")]
    InvalidMetadata(toml::de::Error),
    #[error("The name of category must be legal UTF-8 string")]
    IllegalCategoryName,
}
