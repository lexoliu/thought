#![feature(error_generic_member_access)]

pub mod error;
pub use error::{Error, Result};
pub mod article;
mod config;
pub mod generate;
pub use config::Config;
mod category;
pub use category::Category;
mod utils;
pub mod workspace;
pub use workspace::Workspace;
pub mod metadata;
