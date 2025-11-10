use time::OffsetDateTime;

use crate::{
    article::{Article, ArticlePreview},
    category::Category,
    metadata::{ArticleMetadata, CategoryMetadata},
};

pub mod hook {
    wasmtime::component::bindgen!({
        path: "plugin/wit/plugin.wit",
        world: "hook-runtime",
    });
}

pub mod theme {
    wasmtime::component::bindgen!({
        path: "plugin/wit/plugin.wit",
        world: "theme-runtime",
    });
}
type WITTimestamp = hook::thought::plugin::types::Timestamp;
type WITArticle = hook::thought::plugin::types::Article;
type WITArticlePreview = hook::thought::plugin::types::ArticlePreview;
type WITCategory = hook::thought::plugin::types::Category;
type WITArticleMetadata = hook::thought::plugin::types::ArticleMetadata;
type WITCategoryMetadata = hook::thought::plugin::types::CategoryMetadata;
impl From<&Article> for WITArticle {
    fn from(article: &Article) -> Self {
        WITArticle {
            preview: article.preview().into(),
            content: article.content().to_string(),
        }
    }
}

impl From<OffsetDateTime> for WITTimestamp {
    fn from(datetime: OffsetDateTime) -> Self {
        WITTimestamp {
            seconds: datetime.unix_timestamp(),
            nanos: datetime.nanosecond(),
        }
    }
}

impl From<&ArticleMetadata> for WITArticleMetadata {
    fn from(metadata: &crate::metadata::ArticleMetadata) -> Self {
        WITArticleMetadata {
            created: metadata.created().into(),
            tags: metadata.tags().to_vec(),
            author: metadata.author().to_string(),
            description: metadata.description().map(ToString::to_string),
        }
    }
}

impl From<&ArticlePreview> for WITArticlePreview {
    fn from(article: &ArticlePreview) -> Self {
        WITArticlePreview {
            title: article.title().to_string(),
            slug: article.slug().to_string(),
            category: article.category().into(),
            metadata: article.metadata().into(),
            description: article.description().to_string(),
        }
    }
}

impl From<&Category> for WITCategory {
    fn from(category: &Category) -> Self {
        WITCategory {
            path: category.segments().clone(),
            metadata: category.metadata().into(),
        }
    }
}

impl From<&CategoryMetadata> for WITCategoryMetadata {
    fn from(metadata: &CategoryMetadata) -> Self {
        WITCategoryMetadata {
            created: metadata.created().into(),
            name: metadata.name().to_string(),
            description: metadata.description().to_string(),
        }
    }
}
