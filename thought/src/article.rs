use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::workspace::Workspace;

pub struct Article {
    name: String,
    category: Category,
    sha256: [u8; 32],
    content: String,
    excerpt: String,
    count: u32,
}

pub struct Category(Vec<String>);

pub struct ArticlePath {
    category: Category,
}

impl Article {
    pub fn create(workspace: &Workspace) -> Self {
        todo!()
    }

    pub fn content(&self, workspace: &Workspace) -> String {
        todo!()
    }
}
