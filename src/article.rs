use std::path::PathBuf;

use serde_json::json;
use sha2::Digest;

use crate::{
    category::Category,
    metadata::{ArticleMetadata, FailToOpenMetadata, MetadataExt},
    utils::read_to_string,
    workspace::Workspace,
};

/// An article with its full content
#[derive(Debug, Clone)]
pub struct Article {
    workspace: Workspace,
    content: String, // markdown content
    preview: ArticlePreview,
}

/// A preview of an article without its content
#[derive(Debug, Clone)]
pub struct ArticlePreview {
    title: String,
    slug: String,
    category: Category,
    metadata: ArticleMetadata,
    description: String,
}

impl ArticlePreview {
    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }
    #[must_use]
    pub fn slug(&self) -> &str {
        &self.slug
    }

    #[must_use]
    pub const fn category(&self) -> &Category {
        &self.category
    }

    #[must_use]
    pub const fn metadata(&self) -> &ArticleMetadata {
        &self.metadata
    }

    #[must_use]
    pub fn description(&self) -> &str {
        &self.description
    }
}

impl Article {
    /// Create a new article with the given parameters
    pub async fn create(
        workspace: Workspace,
        title: impl Into<String>,
        slug: impl Into<String>,
        category: Category,
        metadata: ArticleMetadata,
        description: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        Self {
            workspace,
            content: content.into(),
            preview: ArticlePreview {
                title: title.into(),
                slug: slug.into(),
                category,
                metadata,
                description: description.into(),
            },
        }
    }

    // example: /path/to/article.md
    // would be open("/path/to", ["category1", "category2", "article-name"])
    /// Open an article from the given root path and article path
    /// # Errors
    /// Returns `FailToOpenArticle::WorkspaceNotFound` if the workspace does not exist
    /// Returns `FailToOpenArticle::ArticleNotFound` if the article does not exist
    /// Returns `FailToOpenArticle::FailToOpenMetadata` if the metadata file cannot be opened
    #[allow(clippy::missing_panics_doc)]
    pub async fn open(
        workspace: Workspace,
        segments: impl Into<Vec<String>>,
    ) -> Result<Self, FailToOpenArticle> {
        let segments = segments.into();
        let path_buf = workspace.articles_dir();
        let full_path = segments.iter().fold(path_buf, |acc, comp| acc.join(comp));
        let category_path = full_path
            .parent()
            .ok_or(FailToOpenArticle::ArticleNotFound)?;
        let metadata_path = full_path.join("Article.toml");
        let content_path = full_path.join("article.md");

        // check if the article directory exists
        if !full_path.exists() || !full_path.is_dir() {
            return Err(FailToOpenArticle::ArticleNotFound);
        }

        let metadata = ArticleMetadata::open(metadata_path)
            .await
            .map_err(FailToOpenArticle::FailToOpenMetadata)?;

        let content = read_to_string(content_path)
            .await
            .map_err(|_| FailToOpenArticle::ArticleNotFound)?;

        let slug = segments
            .last()
            .ok_or(FailToOpenArticle::ArticleNotFound)?
            .clone();

        let category = Category::open(workspace.clone(), category_path)
            .await
            .map_err(|_| FailToOpenArticle::WorkspaceNotFound)?;

        let extraction = extract(&content);

        Ok(Self {
            workspace,
            content: extraction.content.to_string(),
            preview: ArticlePreview {
                title: extraction.title.unwrap_or_else(|| {
                    // use date of created as title
                    let format = format_description!(
                        "[weekday repr:short] [day padding:none] [month repr:short]"
                    );
                    metadata
                        .created()
                        .format(format)
                        .expect("Failed to format date")
                }),
                slug,
                category,
                metadata,
                description: extraction.description,
            },
        })
    }

    pub fn dir(&self) -> PathBuf {
        self.category().dir().join(self.slug())
    }

    pub fn segments(&self) -> Vec<String> {
        let mut segments = self.category().segments().to_vec();
        segments.push(self.slug().to_string());
        segments
    }

    /// Get a reference to the article preview
    #[must_use]
    pub const fn preview(&self) -> &ArticlePreview {
        &self.preview
    }

    #[must_use]
    pub const fn category(&self) -> &Category {
        &self.preview.category
    }

    #[must_use]
    pub const fn content(&self) -> &str {
        self.content.as_str()
    }

    #[must_use]
    pub const fn slug(&self) -> &str {
        self.preview.slug.as_str()
    }

    #[must_use]
    pub const fn title(&self) -> &str {
        self.preview.title.as_str()
    }

    #[must_use]
    pub const fn description(&self) -> &str {
        self.preview.description.as_str()
    }

    #[must_use]
    pub const fn metadata(&self) -> &ArticleMetadata {
        &self.preview.metadata
    }

    /// Calculate the SHA256 hash of the article
    /// This can be used to uniquely identify the article content
    #[allow(clippy::missing_panics_doc)]
    #[must_use]
    pub fn sha256(&self) -> String {
        // hash of whole article object
        // let's encode whole object to json firstly
        let json = serde_json::to_string(&json!({
            "title": self.title(),
            "slug": self.slug(),
            "category": self.category().dir(),
            "metadata": {
                "created": self.metadata().created().unix_timestamp(),
                "tags": self.metadata().tags(),
                "author": self.metadata().author(),
                "description": self.metadata().description(),
            },
            "description": self.description(),
            "content": self.content(),
        }))
        .expect("Failed to serialize article to JSON");
        let mut hasher = sha2::Sha256::new();
        hasher.update(json.as_bytes());
        let result = hasher.finalize();
        format!("{result:x}")
    }
}

use pulldown_cmark::{Event, Parser, Tag};
use time::macros::format_description;

#[derive(Debug, thiserror::Error)]
pub enum FailToOpenArticle {
    #[error("Workspace not found")]
    WorkspaceNotFound,
    #[error("Article not found")]
    ArticleNotFound,
    #[error("Failed to open metadata")]
    FailToOpenMetadata(FailToOpenMetadata),
}

// extract title,description and content from markdown, but do not render it to html
struct ExtractionResult<'a> {
    title: Option<String>,
    description: String,
    content: &'a str,
}

fn extract(input: &str) -> ExtractionResult<'_> {
    let mut title = None;
    let mut description = String::new();
    let mut in_title_heading = false;
    let mut in_description_paragraph = false;
    let mut description_found = false;

    // Create a new parser. We need to clone it to iterate multiple times.
    let parser = Parser::new(input);

    for event in parser {
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                if level == pulldown_cmark::HeadingLevel::H1 && title.is_none() {
                    in_title_heading = true;
                }
            }
            Event::End(pulldown_cmark::TagEnd::Heading(level)) => {
                if level == pulldown_cmark::HeadingLevel::H1 && in_title_heading {
                    in_title_heading = false;
                }
            }
            Event::Start(Tag::Paragraph) => {
                if title.is_some() && !description_found {
                    in_description_paragraph = true;
                }
            }
            Event::End(pulldown_cmark::TagEnd::Paragraph) => {
                if in_description_paragraph {
                    in_description_paragraph = false;
                    description_found = true;
                }
            }
            Event::Text(text) => {
                if in_title_heading {
                    title = Some(text.into_string());
                } else if in_description_paragraph {
                    description.push_str(&text);
                }
            }
            _ => {}
        }
    }

    ExtractionResult {
        title,
        description,
        content: input,
    }
}
