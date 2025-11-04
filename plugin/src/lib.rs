pub use askama;
pub use pulldown_cmark;
pub mod types {
    wit_bindgen::generate!({
       path: "wit/plugin.wit",
       world: "runtime",
       generate_unused_types:true,

    });
}

#[doc(hidden)]
pub mod theme {
    wit_bindgen::generate!({
       path: "wit/plugin.wit",
       world: "theme-runtime",
       with: { "thought:plugin/types": super::types::thought::plugin::types },
       pub_export_macro: true,
    });
}

pub mod hook {
    wit_bindgen::generate!({
       path: "wit/plugin.wit",
       world: "hook-runtime",
       with: { "thought:plugin/types": super::types::thought::plugin::types },
    generate_unused_types:true,
       pub_export_macro: true,
    });
}

pub use types::thought::plugin::types::*;

pub use hook::export as export_hook;

pub trait Theme {
    fn generate_page(article: Article) -> String;
    fn generate_index(articles: Vec<ArticlePreview>) -> String;
}

impl<T: Theme> theme::exports::thought::plugin::theme::Guest for T {
    fn generate_page(article: Article) -> String {
        <Self as Theme>::generate_page(article)
    }

    fn generate_index(articles: Vec<ArticlePreview>) -> String {
        <Self as Theme>::generate_index(articles)
    }
}

pub trait Hook {
    fn on_pre_render(article: Article) -> Result<Article, String> {
        Ok(article)
    }
    fn on_post_render(article: Article, html: String) -> Result<String, String> {
        let _ = article;
        Ok(html)
    }
}

impl<T: Hook> hook::exports::thought::plugin::hook::Guest for T {
    fn on_post_render(input: Article, html: String) -> String {
        <Self as Hook>::on_post_render(input, html).expect("Hook on_post_render failed")
    }

    fn on_pre_render(input: Article) -> Article {
        <Self as Hook>::on_pre_render(input).expect("Hook on_pre_render failed")
    }
}

#[macro_export]
macro_rules! export_theme {
    ($ty:ident) => {
        $crate::theme::export!($ty with_types_in $crate::theme);
    };
}

use time::{Duration, OffsetDateTime};

impl Timestamp {
    #[must_use]
    pub fn to_offset_datetime(&self) -> OffsetDateTime {
        let base =
            OffsetDateTime::from_unix_timestamp(self.seconds).unwrap_or(OffsetDateTime::UNIX_EPOCH);
        base + Duration::nanoseconds(self.nanos.into())
    }

    #[must_use]
    pub fn from_offset_datetime(datetime: OffsetDateTime) -> Self {
        Self {
            seconds: datetime.unix_timestamp(),
            nanos: datetime.nanosecond(),
        }
    }
}

impl ArticleMetadata {
    #[must_use]
    pub fn created(&self) -> OffsetDateTime {
        self.created.to_offset_datetime()
    }

    #[must_use]
    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    #[must_use]
    pub fn tags(&self) -> &[String] {
        &self.tags
    }

    #[must_use]
    pub fn author(&self) -> &str {
        self.author.as_str()
    }
}

impl ArticlePreview {
    #[must_use]
    pub fn title(&self) -> &str {
        self.title.as_str()
    }

    #[must_use]
    pub fn slug(&self) -> &str {
        self.slug.as_str()
    }

    #[must_use]
    pub fn description(&self) -> &str {
        self.description.as_str()
    }

    #[must_use]
    pub fn metadata(&self) -> &ArticleMetadata {
        &self.metadata
    }

    #[must_use]
    pub fn category(&self) -> &Category {
        &self.category
    }
}

impl Article {
    #[must_use]
    pub fn content(&self) -> &str {
        self.content.as_str()
    }

    #[must_use]
    pub fn title(&self) -> &str {
        self.preview.title()
    }

    #[must_use]
    pub fn slug(&self) -> &str {
        self.preview.slug()
    }

    #[must_use]
    pub fn metadata(&self) -> &ArticleMetadata {
        self.preview.metadata()
    }

    #[must_use]
    pub fn preview(&self) -> &ArticlePreview {
        &self.preview
    }
}

impl Category {
    #[must_use]
    pub fn path(&self) -> &[String] {
        &self.path
    }

    #[must_use]
    pub fn metadata(&self) -> &CategoryMetadata {
        &self.metadata
    }

    #[must_use]
    pub fn path_string(&self) -> String {
        self.path.join("/")
    }
}

impl CategoryMetadata {
    #[must_use]
    pub fn created(&self) -> OffsetDateTime {
        self.created.to_offset_datetime()
    }

    #[must_use]
    pub fn name(&self) -> &str {
        self.name.as_str()
    }

    #[must_use]
    pub fn description(&self) -> &str {
        self.description.as_str()
    }
}

pub mod helpers {
    use crate::pulldown_cmark::{html, Parser};
    use crate::{Article, ArticlePreview};
    use std::fmt;
    use time::{format_description, format_description::well_known, OffsetDateTime};

    /// Render a Markdown string into HTML.
    #[must_use]
    pub fn markdown_to_html(markdown: &str) -> String {
        let parser = Parser::new(markdown);
        let mut html_output = String::new();
        html::push_html(&mut html_output, parser);
        html_output
    }

    /// Join the category path segments for an article preview.
    #[must_use]
    pub fn article_path(article: &ArticlePreview) -> String {
        article.category().path_string()
    }

    /// Build the relative output path (without extension) for an article.
    #[must_use]
    pub fn article_output_path(article: &ArticlePreview) -> String {
        let mut path = article.category().path().to_vec();
        path.push(article.slug().to_string());
        path.join("/")
    }

    /// Build the relative output file (with `.html`) for an article.
    #[must_use]
    pub fn article_output_file(article: &ArticlePreview) -> String {
        format!("{}.html", article_output_path(article))
    }

    /// Build a permalink by combining a base URL and the relative article path.
    #[must_use]
    pub fn article_permalink(base_url: &str, article: &ArticlePreview) -> String {
        let mut base = base_url.to_string();
        if !base.ends_with('/') {
            base.push('/');
        }
        base.push_str(&article_output_file(article));
        base
    }

    /// Format an [`OffsetDateTime`] using RFC3339.
    #[must_use]
    pub fn format_rfc3339(datetime: OffsetDateTime) -> String {
        datetime
            .format(&well_known::Rfc3339)
            .expect("failed to format datetime as RFC3339")
    }

    /// Format an [`OffsetDateTime`] with a custom format string.
    ///
    /// The format syntax follows the rules of the [`time`] crate.
    pub fn format_datetime(
        datetime: OffsetDateTime,
        format: &str,
    ) -> Result<String, FormatDatetimeError> {
        let parsed =
            format_description::parse(format).map_err(FormatDatetimeError::InvalidFormat)?;
        datetime
            .format(&parsed)
            .map_err(FormatDatetimeError::Format)
    }

    /// Extract the HTML content of an article using helper conversions.
    #[must_use]
    pub fn article_content_html(article: &Article) -> String {
        markdown_to_html(article.content())
    }

    /// Errors that can occur when formatting a datetime with a custom pattern.
    #[derive(Debug)]
    pub enum FormatDatetimeError {
        InvalidFormat(time::error::InvalidFormatDescription),
        Format(time::error::Format),
    }

    impl fmt::Display for FormatDatetimeError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                Self::InvalidFormat(err) => write!(f, "invalid format description: {err}"),
                Self::Format(err) => write!(f, "failed to format datetime: {err}"),
            }
        }
    }

    impl std::error::Error for FormatDatetimeError {}
}

impl From<OffsetDateTime> for Timestamp {
    fn from(datetime: OffsetDateTime) -> Self {
        Self {
            seconds: datetime.unix_timestamp(),
            nanos: datetime.nanosecond(),
        }
    }
}
