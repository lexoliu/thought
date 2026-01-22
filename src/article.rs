use std::path::{Path, PathBuf};

use serde_json::json;
use sha2::Digest;
use whatlang::{Lang, detect};

use crate::{
    category::Category,
    metadata::{ArticleMetadata, FailToOpenMetadata, MetadataExt},
    utils::read_to_string,
    workspace::Workspace,
};

/// An article with its full content
#[derive(Debug, Clone)]
pub struct Article {
    pub(crate) content: String, // markdown content
    pub(crate) preview: ArticlePreview,
}

/// A preview of an article without its content
#[derive(Debug, Clone)]
pub struct ArticlePreview {
    pub(crate) title: String,
    pub(crate) slug: String,
    pub(crate) category: Category,
    pub(crate) metadata: ArticleMetadata,
    pub(crate) description: String,
    pub(crate) locale: String,
    pub(crate) default_locale: String,
    pub(crate) translations: Vec<ArticleTranslation>,
}

#[derive(Debug, Clone)]
pub struct ArticleTranslation {
    pub(crate) locale: String,
    pub(crate) title: String,
}

impl ArticleTranslation {
    #[must_use]
    pub fn locale(&self) -> &str {
        &self.locale
    }

    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }
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

    #[must_use]
    pub fn locale(&self) -> &str {
        &self.locale
    }

    #[must_use]
    pub fn default_locale(&self) -> &str {
        &self.default_locale
    }

    #[must_use]
    pub fn is_default_locale(&self) -> bool {
        self.locale == self.default_locale
    }

    #[must_use]
    pub fn translations(&self) -> &[ArticleTranslation] {
        &self.translations
    }

    #[must_use]
    pub fn output_path(&self) -> String {
        let mut path = self.category().segments().to_vec();
        let mut slug = self.slug().to_string();
        if !self.is_default_locale() {
            slug.push('.');
            slug.push_str(self.locale());
        }
        path.push(slug);
        path.join("/")
    }

    #[must_use]
    pub fn output_file(&self) -> String {
        format!("{}.html", self.output_path())
    }
}

impl Article {
    /// Create a new article with the given parameters
    pub async fn create(
        title: impl Into<String>,
        slug: impl Into<String>,
        category: Category,
        metadata: ArticleMetadata,
        description: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        let content = content.into();
        let default_locale = resolve_default_locale(metadata.lang(), &content);
        let title = title.into();
        let slug = slug.into();
        let description = description.into();
        let translations = vec![ArticleTranslation {
            locale: default_locale.clone(),
            title: title.clone(),
        }];
        Self {
            content,
            preview: ArticlePreview {
                title,
                slug,
                category,
                metadata,
                description,
                locale: default_locale.clone(),
                default_locale,
                translations,
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
        Self::open_with_locale(workspace, segments, None).await
    }

    /// Open an article for a specific locale. If `locale` is `None`, the default locale is used.
    pub async fn open_with_locale(
        workspace: Workspace,
        segments: impl Into<Vec<String>>,
        locale: Option<String>,
    ) -> Result<Self, FailToOpenArticle> {
        let segments = segments.into();
        let path_buf = workspace.articles_dir();
        let full_path = segments.iter().fold(path_buf, |acc, comp| acc.join(comp));
        let category_path = full_path
            .parent()
            .ok_or(FailToOpenArticle::ArticleNotFound)?;

        // check if the article directory exists
        if !full_path.exists() || !full_path.is_dir() {
            return Err(FailToOpenArticle::ArticleNotFound);
        }

        let metadata_path = full_path.join("Article.toml");
        let metadata = ArticleMetadata::open(metadata_path)
            .await
            .map_err(FailToOpenArticle::FailToOpenMetadata)?;
        let default_locale = resolve_default_locale_from_disk(&full_path, metadata.lang()).await?;

        let available = enumerate_locales(&full_path, &default_locale).await?;
        let target_locale = locale.unwrap_or_else(|| default_locale.clone());
        let content_path = locale_to_path(&full_path, &target_locale, &default_locale);

        if !content_path.exists() {
            return Err(FailToOpenArticle::ArticleNotFound);
        }

        let content = read_to_string(&content_path)
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
        let title = extraction.title.unwrap_or_else(|| {
            let format =
                format_description!("[weekday repr:short] [day padding:none] [month repr:short]");
            metadata
                .created()
                .format(format)
                .expect("Failed to format date")
        });

        let translations = available
            .iter()
            .map(|variant| ArticleTranslation {
                locale: variant.locale.clone(),
                title: variant.title.clone().unwrap_or_else(|| title.clone()),
            })
            .collect::<Vec<_>>();

        Ok(Self {
            content: extraction.content.to_string(),
            preview: ArticlePreview {
                title,
                slug,
                category,
                metadata,
                description: extraction.description,
                locale: target_locale,
                default_locale,
                translations,
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

    #[must_use]
    pub fn locale(&self) -> &str {
        self.preview.locale()
    }

    #[must_use]
    pub fn default_locale(&self) -> &str {
        self.preview.default_locale()
    }

    #[must_use]
    pub fn is_default_locale(&self) -> bool {
        self.preview.is_default_locale()
    }

    #[must_use]
    pub fn translations(&self) -> &[ArticleTranslation] {
        self.preview.translations()
    }

    #[must_use]
    pub fn output_path(&self) -> String {
        self.preview.output_path()
    }

    #[must_use]
    pub fn output_file(&self) -> String {
        self.preview.output_file()
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
            "locale": self.locale(),
            "metadata": {
                "created": self.metadata().created().unix_timestamp(),
                "tags": self.metadata().tags(),
                "author": self.metadata().author(),
                "description": self.metadata().description(),
            },
            "description": self.description(),
            "content": self.content(),
            "translations": self
                .translations()
                .iter()
                .map(|t| json!({"locale": t.locale(), "title": t.title()}))
                .collect::<Vec<_>>(),
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

fn resolve_default_locale(metadata_lang: Option<&str>, content: &str) -> String {
    if let Some(lang) = normalize_lang_tag(metadata_lang) {
        return lang;
    }
    detect_locale_from_text(content).unwrap_or_else(|| "en".to_string())
}

async fn resolve_default_locale_from_disk(
    article_dir: &Path,
    metadata_lang: Option<&str>,
) -> Result<String, FailToOpenArticle> {
    if let Some(lang) = normalize_lang_tag(metadata_lang) {
        return Ok(lang);
    }

    let primary = article_dir.join("article.md");
    if let Ok(content) = read_to_string(&primary).await {
        if let Some(lang) = detect_locale_from_text(&content) {
            return Ok(lang);
        }
    }

    let mut entries = tokio::fs::read_dir(article_dir)
        .await
        .map_err(|_| FailToOpenArticle::ArticleNotFound)?;
    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|_| FailToOpenArticle::ArticleNotFound)?
    {
        if entry
            .file_type()
            .await
            .map_err(|_| FailToOpenArticle::ArticleNotFound)?
            .is_file()
        {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("md") {
                continue;
            }
            if let Ok(content) = read_to_string(&path).await {
                if let Some(lang) = detect_locale_from_text(&content) {
                    return Ok(lang);
                }
            }
        }
    }

    Ok("en".to_string())
}

fn detect_locale_from_text(text: &str) -> Option<String> {
    let info = detect(text)?;
    if !(info.is_reliable() || info.confidence() >= 0.5) {
        return None;
    }
    Some(lang_to_locale(info.lang()))
}

fn normalize_lang_tag(lang: Option<&str>) -> Option<String> {
    lang.and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

fn lang_to_locale(lang: Lang) -> String {
    match lang {
        Lang::Eng => "en",
        Lang::Cmn => "zh",
        Lang::Spa => "es",
        Lang::Por => "pt",
        Lang::Fra => "fr",
        Lang::Deu => "de",
        Lang::Rus => "ru",
        Lang::Ukr => "uk",
        Lang::Jpn => "ja",
        Lang::Kor => "ko",
        Lang::Ara => "ar",
        Lang::Hin => "hi",
        Lang::Ben => "bn",
        Lang::Ita => "it",
        Lang::Nld => "nl",
        Lang::Swe => "sv",
        Lang::Fin => "fi",
        Lang::Tur => "tr",
        Lang::Pol => "pl",
        Lang::Vie => "vi",
        Lang::Tha => "th",
        Lang::Heb => "he",
        Lang::Cat => "ca",
        Lang::Ron => "ro",
        Lang::Ces => "cs",
        Lang::Ell => "el",
        Lang::Hun => "hu",
        Lang::Dan => "da",
        Lang::Nob => "nb",
        Lang::Bul => "bg",
        Lang::Bel => "be",
        Lang::Mar => "mr",
        Lang::Kan => "kn",
        Lang::Tam => "ta",
        Lang::Urd => "ur",
        Lang::Uzb => "uz",
        Lang::Aze => "az",
        Lang::Ind => "id",
        Lang::Tel => "te",
        Lang::Pes => "fa",
        Lang::Mal => "ml",
        Lang::Ori => "or",
        Lang::Mya => "my",
        Lang::Nep => "ne",
        Lang::Sin => "si",
        Lang::Khm => "km",
        Lang::Tuk => "tk",
        Lang::Aka => "ak",
        Lang::Zul => "zu",
        Lang::Sna => "sn",
        Lang::Afr => "af",
        Lang::Lat => "la",
        Lang::Slk => "sk",
        Lang::Tgl => "tl",
        Lang::Hrv => "hr",
        Lang::Srp => "sr",
        Lang::Mkd => "mk",
        Lang::Lit => "lt",
        Lang::Lav => "lv",
        Lang::Est => "et",
        Lang::Amh => "am",
        Lang::Jav => "jv",
        Lang::Pan => "pa",
        Lang::Kat => "ka",
        Lang::Hye => "hy",
        Lang::Yid => "yi",
        _ => lang.code(),
    }
    .to_string()
}

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

#[derive(Debug, Clone)]
struct LocaleVariant {
    locale: String,
    title: Option<String>,
}

fn parse_locale_from_filename(path: &Path, default_locale: &str) -> Option<String> {
    if path.extension().and_then(|ext| ext.to_str()) != Some("md") {
        return None;
    }
    match path.file_stem().and_then(|stem| stem.to_str()) {
        Some("article") => Some(default_locale.to_string()),
        Some(stem) if !stem.is_empty() => Some(stem.to_string()),
        _ => None,
    }
}

async fn enumerate_locales(
    dir: &Path,
    default_locale: &str,
) -> Result<Vec<LocaleVariant>, FailToOpenArticle> {
    let mut entries = tokio::fs::read_dir(dir)
        .await
        .map_err(|_| FailToOpenArticle::ArticleNotFound)?;
    let mut variants = Vec::new();
    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|_| FailToOpenArticle::ArticleNotFound)?
    {
        let path = entry.path();
        if !entry
            .file_type()
            .await
            .map_err(|_| FailToOpenArticle::ArticleNotFound)?
            .is_file()
        {
            continue;
        }

        let Some(locale) = parse_locale_from_filename(&path, default_locale) else {
            continue;
        };
        let content = read_to_string(&path)
            .await
            .map_err(|_| FailToOpenArticle::ArticleNotFound)?;
        let extraction = extract(&content);
        variants.push(LocaleVariant {
            locale,
            title: extraction.title.map(|s| s.to_string()),
        });
    }

    if variants.is_empty() {
        Err(FailToOpenArticle::ArticleNotFound)
    } else {
        Ok(variants)
    }
}

fn locale_to_path(dir: &Path, locale: &str, default_locale: &str) -> PathBuf {
    if locale == default_locale {
        return dir.join("article.md");
    }
    dir.join(format!("{locale}.md"))
}
