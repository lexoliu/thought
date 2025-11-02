use alloc::string::String;
use serde::Serialize;
use sha2::Digest;

use crate::{category::Category, metadata::ArticleMetadata};

/// An article with its full content
#[derive(Debug, Clone, Serialize, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct Article {
    content: String, // markdown content
    #[serde(flatten)]
    preview: ArticlePreview,
}

/// A preview of an article without its content
#[derive(Debug, Clone, Serialize, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct ArticlePreview {
    title: String,
    slug: String,
    category: Category,
    metadata: ArticleMetadata,
    description: String,
}

impl Article {
    /// Create a new article with the given parameters
    #[must_use]
    pub const fn new(
        title: String,
        slug: String,
        category: Category,
        metadata: ArticleMetadata,
        description: String,
        content: String,
    ) -> Self {
        Self {
            content,
            preview: ArticlePreview {
                title,
                slug,
                category,
                metadata,
                description,
            },
        }
    }

    /// Get a reference to the article preview
    #[must_use]
    pub const fn preview(&self) -> &ArticlePreview {
        &self.preview
    }

    /// Consume the article and return its preview
    #[must_use]
    pub fn into_preview(self) -> ArticlePreview {
        self.preview
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
        let json = serde_json::to_string(self).expect("Failed to serialize article to JSON");
        let mut hasher = sha2::Sha256::new();
        hasher.update(json.as_bytes());
        let result = hasher.finalize();
        alloc::format!("{result:x}")
    }
}

mod io {
    use std::{
        path::Path,
        string::{String, ToString},
        vec::Vec,
    };

    use crate::{article::Article, metadata::MetadataExt};
    use pulldown_cmark::{Event, Parser, Tag};
    use time::macros::format_description;

    #[derive(Debug, thiserror::Error)]
    pub enum FailToOpenArticle {
        #[error("Workspace not found")]
        WorkspaceNotFound,
        #[error("Article not found")]
        ArticleNotFound,
        #[error("Failed to open metadata")]
        FailToOpenMetadata(crate::metadata::FailToOpenMetadata),
    }

    impl Article {
        // example: /path/to/article.md
        // would be open("/path/to", ["category1", "category2", "article-name"])
        /// Open an article from the given root path and article path
        /// # Errors
        /// Returns `FailToOpenArticle::WorkspaceNotFound` if the workspace does not exist
        /// Returns `FailToOpenArticle::ArticleNotFound` if the article does not exist
        /// Returns `FailToOpenArticle::FailToOpenMetadata` if the metadata file cannot be opened
        #[allow(clippy::missing_panics_doc)]
        pub async fn open(
            root: impl AsRef<Path>,
            path: impl Into<Vec<String>>,
        ) -> Result<Self, FailToOpenArticle> {
            let path_vec = path.into();
            let path_buf = root.as_ref().join("articles");
            let full_path = path_vec.iter().fold(path_buf, |acc, comp| acc.join(comp));
            let metadata_path = full_path.join("Article.toml");
            let content_path = full_path.join("article.md");

            // check if the article directory exists
            if !full_path.exists() || !full_path.is_dir() {
                return Err(FailToOpenArticle::ArticleNotFound);
            }

            let metadata = crate::metadata::ArticleMetadata::open(metadata_path)
                .await
                .map_err(FailToOpenArticle::FailToOpenMetadata)?;

            let content = std::fs::read_to_string(content_path)
                .map_err(|_| FailToOpenArticle::ArticleNotFound)?;

            let slug = path_vec
                .last()
                .ok_or(FailToOpenArticle::ArticleNotFound)?
                .clone();

            let category_path = &path_vec[..path_vec.len() - 1];
            let category = crate::category::Category::open(root, category_path.to_vec())
                .await
                .map_err(|_| FailToOpenArticle::WorkspaceNotFound)?;

            let extraction = extract(&content);

            Ok(Self {
                content: extraction.content.to_string(),
                preview: crate::article::ArticlePreview {
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
                Event::Start(Tag::Heading(level, _, _)) => {
                    if level == pulldown_cmark::HeadingLevel::H1 && title.is_none() {
                        in_title_heading = true;
                    }
                }
                Event::End(Tag::Heading(level, _, _)) => {
                    if level == pulldown_cmark::HeadingLevel::H1 && in_title_heading {
                        in_title_heading = false;
                    }
                }
                Event::Start(Tag::Paragraph) => {
                    if title.is_some() && !description_found {
                        in_description_paragraph = true;
                    }
                }
                Event::End(Tag::Paragraph) => {
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
}
