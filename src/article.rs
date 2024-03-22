use std::fs::create_dir;
use std::io;
use std::ops::Deref;
use std::path::{Path, PathBuf};

use liquid::object;
use pulldown_cmark::{Event, HeadingLevel, Parser, Tag};

use crate::build::BuildResource;
use crate::category::Category;
use crate::metadata::ArticleMetadata;
use crate::utils::{create_file, not_found, read_to_string, to_html};
use crate::workspace::Workspace;
use crate::{Error, Result};

#[derive(Debug, Clone)]
pub struct Article {
    content: String,
    preview: ArticlePreview,
}

impl Deref for Article {
    type Target = ArticlePreview;
    fn deref(&self) -> &Self::Target {
        self.preview()
    }
}

#[derive(Debug, Clone)]
pub struct ArticlePreview {
    title: String,
    name: String,
    category: Category,
    metadata: ArticleMetadata,
    description: String,
}

impl ArticlePreview {
    pub fn open(category: Category, name: String) -> Result<Self> {
        let path = category.workspace().article_path(&name, &category);
        let content = not_found(
            read_to_string(path.join("article.md")),
            Error::ArticleNotFound,
        )?;

        let metadata = ArticleMetadata::open(path.join("metadata.toml"))?;
        let mut title = Vec::new();

        let mut content = Parser::new(&content);

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

        let description = String::new();
        let mut buf = Vec::new();
        let mut count: u32 = 0;
        for event in content {
            count += count_event(event.clone());
            if count >= 200 {
                break;
            }
            buf.push(event);
        }

        Ok(Self {
            title,
            category,
            metadata,
            name,
            description,
        })
    }

    pub fn from_dir(workspace: Workspace, path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();

        let name = path.file_name().ok_or(Error::ArticleNotFound)?;
        let name = String::from_utf8(name.as_encoded_bytes().to_vec())?;

        let category = path
            .strip_prefix(workspace.path())
            .map_err(|_| Error::ArticleNotFound)?
            .parent()
            .ok_or(Error::ArticleNotFound)?;

        Self::open(Category::from_dir(workspace, category)?, name)
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub const fn category(&self) -> &Category {
        &self.category
    }

    pub const fn metadata(&self) -> &ArticleMetadata {
        &self.metadata
    }

    pub fn description(&self) -> &str {
        &self.description
    }

    pub fn path(&self) -> PathBuf {
        let mut path = self.category.workspace().path().join("articles");
        path.extend(&self.category);
        path.push(&self.name);
        path
    }

    pub fn detail(self) -> Result<Article> {
        let content = read_to_string(self.path().join("article.md"))?;

        Ok(Article {
            content,
            preview: self,
        })
    }
}

impl Article {
    pub fn open(category: Category, name: String) -> Result<Self> {
        ArticlePreview::open(category, name)?.detail()
    }

    pub fn create(category: Category, name: String) -> Result<Self> {
        let workspace = category.workspace();
        let path = category.workspace().article_path(&name, &category);

        create_dir(&path).map_err(|error| {
            if error.kind() == io::ErrorKind::AlreadyExists {
                Error::PostAlreadyExists
            } else {
                error.into()
            }
        })?;

        let metadata = ArticleMetadata::new(workspace.config().owner());
        create_file(path.join(".metadata.toml"), metadata.export())?;

        create_file(path.join("article.md"), "# \n")?;
        Ok(Article {
            content: String::new(),
            preview: ArticlePreview {
                title: String::new(),
                name,
                category,
                metadata,
                description: String::new(),
            },
        })
    }

    pub fn description(&self) -> &str {
        self.preview.description()
    }
    pub const fn preview(&self) -> &ArticlePreview {
        &self.preview
    }

    pub const fn metadata(&self) -> &ArticleMetadata {
        &self.preview.metadata
    }

    pub fn render(&self, resource: &BuildResource) -> Result<String> {
        let created = self.metadata().created();
        Ok(resource.article_template.render(&object!({
            "title":self.title,
            "content":self.content,
            "created":format!("{}.{}.{}",created.year(),created.month() as u8,created.day()),
            "author":self.metadata.author(),
            "tags":self.metadata.tags(),
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
