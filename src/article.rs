use std::fs::{create_dir, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use liquid::object;
use pulldown_cmark::{Event, HeadingLevel, Parser, Tag};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::build::BuildResource;
use crate::category::Category;
use crate::utils::{create_file, not_found, read_to_string, to_html};
use crate::workspace::Workspace;
use crate::{Error, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Metadata {
    #[serde(with = "time::serde::rfc3339")]
    created: OffsetDateTime,
    author: String,
    tags: Vec<String>,
}

impl Metadata {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let metadata = not_found(read_to_string(path), Error::ArticleNotFound)?;
        toml::from_str(&metadata).map_err(Error::InvalidMetadata)
    }

    pub fn new(created: OffsetDateTime, author: impl Into<String>) -> Self {
        Self {
            created,
            author: author.into(),
            tags: Vec::new(),
        }
    }

    pub fn created(&self) -> OffsetDateTime {
        self.created
    }

    pub fn author(&self) -> &str {
        &self.author
    }

    pub fn set_author(&mut self, author: impl Into<String>) {
        self.author = author.into();
    }

    pub fn tags(&self) -> &[String] {
        &self.tags
    }

    pub fn add_tag(&mut self, tag: impl Into<String>) {
        self.tags.push(tag.into());
    }

    pub fn export(&self) -> String {
        // Serialization for config never fail, so that we can use `unwrap` silently.
        toml::to_string_pretty(&self).unwrap()
    }

    pub fn save(&self, path: impl AsRef<Path>) -> std::io::Result<()> {
        File::open(path)?.write_all(self.export().as_bytes())
    }
}

#[derive(Debug)]
pub struct Article {
    workspace: Workspace,
    title: String,
    category: Category,
    content: String,
    metadata: Metadata,
    name: String,
}

#[derive(Debug, Serialize)]
pub struct ArticlePreview {
    pub title: String,
    pub description: String,
    pub metadata: Metadata,
}

impl Article {
    pub fn open(workspace: Workspace, name: String, category: Category) -> Result<Self> {
        let mut path = workspace.path().to_owned();
        path.extend(&category);
        path.push(&name);
        let content = not_found(
            read_to_string(path.join("article.md")),
            Error::ArticleNotFound,
        )?;

        let metadata = Metadata::open(path.join("metadata.toml"))?;
        let mut content = Parser::new(&content);
        let mut title = Vec::new();

        if let Some(Event::Start(Tag::Heading(level, _, _))) = content.next() {
            if level == HeadingLevel::H1 {
                for event in content.by_ref() {
                    if let Event::End(Tag::Heading(_, _, _)) = event {
                        break;
                    } else {
                        title.push(event);
                    }
                }
            }
        }

        let title = to_html(title);
        let content = to_html(content);

        Ok(Self {
            workspace,
            title,
            category,
            content,
            metadata,
            name,
        })
    }

    pub fn create(workspace: Workspace, name: String, category: Category) -> Result<Self> {
        let mut path = workspace.path().join("articles");
        path.extend(&category);
        path.push(&name);

        create_dir(&path).map_err(|error| {
            if error.kind() == io::ErrorKind::AlreadyExists {
                Error::PostAlreadyExists
            } else {
                error.into()
            }
        })?;

        let metadata = Metadata::new(OffsetDateTime::now_utc(), workspace.config().owner());
        create_file(path.join(".metadata.toml"), metadata.export())?;

        create_file(path.join("article.md"), "# \n")?;
        Ok(Article {
            workspace,
            title: "".into(),
            category,
            content: "".into(),
            metadata,
            name,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub const fn category(&self) -> &Category {
        &self.category
    }

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

    pub fn path(&self) -> PathBuf {
        let mut path = self.workspace.path().join("articles");
        path.extend(&self.category);
        path.push(&self.name);
        path
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
}

fn count_event(event: pulldown_cmark::Event<'_>) -> u32 {
    match event {
        pulldown_cmark::Event::Text(text) => text.len() as u32,
        _ => 0,
    }
}
