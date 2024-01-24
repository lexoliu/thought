use std::path::Path;

use itertools::Itertools;
use liquid::object;
use pulldown_cmark::{Event, HeadingLevel, Tag};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::utils::{read_to_string, to_html, workspace, BuildResource, Error, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Metadata {
    #[serde(with = "time::serde::rfc3339")]
    pub created: OffsetDateTime,
    pub tags: Vec<String>,
}

impl Metadata {
    pub const fn new(created: OffsetDateTime) -> Self {
        Self {
            created,
            tags: Vec::new(),
        }
    }
}

pub struct Article {
    pub title: String,
    pub category: Vec<String>,
    pub content: String,
    pub metadata: Metadata,
    pub name: String,
}

#[derive(Debug, Serialize)]
pub struct ArticlePreview {
    pub title: String,
    pub description: String,
    pub metadata: Metadata,
    pub url: String,
}

impl Article {
    // This article must be in a workspace.
    pub fn from_dir(path: impl AsRef<Path>) -> Result<Article> {
        let workspace = workspace()?;
        let path = path.as_ref();
        let category = path
            .strip_prefix(workspace.join("articles"))
            .map_err(|_| Error::WorkspaceNotFound)?
            .parent()
            .into_iter();
        let category = category
            .map(|v| {
                v.to_str()
                    .map(String::from)
                    .ok_or(Error::IllegalCategoryName)
            })
            .try_collect()?;

        let content = read_to_string(path.join("article.md"))?;

        let metadata = read_to_string(path.join(".metadata.toml"))?;

        let metadata: Metadata = toml::from_str(&metadata).map_err(Error::InvalidMetadata)?;
        let name = path.file_name().unwrap().to_str().unwrap().to_string();

        let mut parser = pulldown_cmark::Parser::new(&content);
        let mut title = Vec::new();

        if let Some(Event::Start(Tag::Heading(level, _, _))) = parser.next() {
            if level == HeadingLevel::H1 {
                for event in parser.by_ref() {
                    if let Event::End(Tag::Heading(_, _, _)) = event {
                        break;
                    } else {
                        title.push(event);
                    }
                }
            }
        }

        let content = to_html(parser);

        Ok(Article {
            title: to_html(title),
            category,
            content,
            name,
            metadata,
        })
    }

    pub fn preview(&self) -> ArticlePreview {
        ArticlePreview {
            title: self.title.clone(),

            description: preview_description(&self.content),

            metadata: self.metadata.clone(),
            url: format!(".{}/{}", self.category.join("/"), self.name),
        }
    }

    pub fn render(&self, resource: &BuildResource) -> std::result::Result<String, liquid::Error> {
        let created = self.metadata.created;
        resource.article_template.render(&object!({
            "title":self.title,
            "content":self.content,
            "created":format!("{}.{}.{}",created.year(),created.month() as u8,created.day()),
            "author":resource.config.author,
            "tags":self.metadata.tags,
            "footer":resource.footer,
        }))
    }
}

fn preview_description(content: &str) -> String {
    let parser = pulldown_cmark::Parser::new(content);
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

fn count_event(event: pulldown_cmark::Event<'_>) -> u32 {
    match event {
        pulldown_cmark::Event::Text(text) => text.len() as u32,
        _ => 0,
    }
}

#[test]
fn test() {
    let parser = pulldown_cmark::Parser::new("# 233\ntest");
    println!("{:?}", parser.collect_vec());
}
