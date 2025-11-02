use rkyv::Archive;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::category::Category;
use crate::util::{thought_alloc, thought_get_article, thought_get_article_content};

#[derive(Debug, Clone)]
pub struct Article {
    id: usize,
    title: String,
    name: String,
    category: Category,
    excerpt: String,
    created: i64, // UNIX TIMESTAMP
    tags: Vec<String>,
    author: String,
    count: usize,
}

impl Article {
    pub fn content(&self) -> String {
        unsafe { String::from_utf8_unchecked(thought_get_article_content(self.id).into_vec()) }
    }
    pub fn id(&self) -> ArticleId {
        ArticleId(self.id)
    }
}

pub fn get_article(id: ArticleId) -> Article {
    // unsafe { thought_get_article(id) }
    todo!()
}
