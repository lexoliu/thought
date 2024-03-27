use std::{backtrace::Backtrace, string::FromUtf8Error};
use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[cfg(debug_assertions)]
    #[error("Inner I/O error: {source}\n Backtrace: {backtrace}")]
    Io {
        #[from]
        source: std::io::Error,
        backtrace: Backtrace,
    },

    #[cfg(not(debug_assertions))]
    #[error("Inner I/O error: {source}")]
    Io {
        #[from]
        source: std::io::Error,
    },
    #[error("Cannot parse template: {0}")]
    Templte(#[from] tera::Error),
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
    #[error("Path includes illegal character")]
    Utf8Error(#[from] FromUtf8Error),
    #[error("Almost done...Please install a theme for Thought.")]
    NeedInstallTemplate,
    #[error("Cannot found template `{name}`")]
    TemplateNotFound { name: String },
}
