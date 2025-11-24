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
        with: {
            "thought:plugin/types":super::hook::thought::plugin::types,
        },
        world: "theme-runtime",
    });
}
pub type WITTimestamp = hook::thought::plugin::types::Timestamp;
pub type WITArticle = hook::thought::plugin::types::Article;
pub type WITArticlePreview = hook::thought::plugin::types::ArticlePreview;
pub type WITCategory = hook::thought::plugin::types::Category;
pub type WITArticleMetadata = hook::thought::plugin::types::ArticleMetadata;
pub type WITCategoryMetadata = hook::thought::plugin::types::CategoryMetadata;
pub type WITTranslation = hook::thought::plugin::types::Translation;
impl From<Article> for WITArticle {
    fn from(article: Article) -> Self {
        WITArticle {
            preview: article.preview.into(),
            content: article.content,
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

impl From<ArticleMetadata> for WITArticleMetadata {
    fn from(metadata: ArticleMetadata) -> Self {
        WITArticleMetadata {
            created: metadata.created.into(),
            tags: metadata.tags,
            author: metadata.author,
            description: metadata.description,
            lang: metadata.lang,
        }
    }
}

impl From<ArticlePreview> for WITArticlePreview {
    fn from(article: ArticlePreview) -> Self {
        WITArticlePreview {
            title: article.title,
            slug: article.slug,
            category: article.category.into(),
            metadata: article.metadata.into(),
            description: article.description,
            locale: article.locale,
            default_locale: article.default_locale,
            translations: article
                .translations
                .into_iter()
                .map(|t| WITTranslation {
                    locale: t.locale,
                    title: t.title,
                })
                .collect(),
        }
    }
}

impl From<Category> for WITCategory {
    fn from(category: Category) -> Self {
        WITCategory {
            path: category.segments,
            metadata: category.metadata.into(),
        }
    }
}

impl From<CategoryMetadata> for WITCategoryMetadata {
    fn from(metadata: CategoryMetadata) -> Self {
        WITCategoryMetadata {
            created: metadata.created.into(),
            name: metadata.name,
            description: metadata.description,
        }
    }
}
