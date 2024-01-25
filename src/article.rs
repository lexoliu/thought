use std::path::PathBuf;

use liquid::object;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::build::BuildResource;
use crate::category::Category;
use crate::workspace::Workspace;
use crate::Result;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Metadata {
    #[serde(with = "time::serde::rfc3339")]
    pub created: OffsetDateTime,
    pub author: String,
    pub tags: Vec<String>,
}

impl Metadata {
    pub fn new(created: OffsetDateTime, author: impl Into<String>) -> Self {
        Self {
            created,
            author: author.into(),
            tags: Vec::new(),
        }
    }

    pub fn add_tag(&mut self, tag: impl Into<String>) {
        self.tags.push(tag.into());
    }

    pub fn export(&self) -> String {
        // Serialization for config never fail, so that we can use `unwrap`
        toml::to_string_pretty(&self).unwrap()
    }
}

#[derive(Debug)]
pub struct Article {
    pub(crate) workspace: Workspace,
    pub title: String,
    pub category: Category,
    pub content: String,
    pub metadata: Metadata,
    pub name: String,
}

#[derive(Debug, Serialize)]
pub struct ArticlePreview {
    pub title: String,
    pub description: String,
    pub metadata: Metadata,
}

impl Article {
    pub fn description(&self) -> String {
        let parser = pulldown_cmark::Parser::new(&self.content);
        let description = String::new();
        let mut buf = Vec::new();
        let mut count: u32 = 0;
        for event in parser {
            count += count_event(event.clone());
            if count >= 200 {
                break;
            }
            buf.push(event);
        }

        description
    }
    pub fn preview(&self) -> ArticlePreview {
        ArticlePreview {
            title: self.title.clone(),
            description: self.description(),
            metadata: self.metadata.clone(),
        }
    }

    pub fn render(&self, resource: &BuildResource) -> Result<String> {
        let created = self.metadata.created;
        Ok(resource.article_template.render(&object!({
            "title":self.title,
            "content":self.content,
            "created":format!("{}.{}.{}",created.year(),created.month() as u8,created.day()),
            "author":self.metadata.author,
            "tags":self.metadata.tags,
            "footer":resource.footer,
        }))?)
    }

    pub fn path(&self) -> PathBuf {
        let mut path = self.workspace.path().join("articles");
        path.extend(&self.category);
        path.push(&self.name);
        path
    }
}

fn count_event(event: pulldown_cmark::Event<'_>) -> u32 {
    match event {
        pulldown_cmark::Event::Text(text) => text.len() as u32,
        _ => 0,
    }
}
