use std::borrow::ToOwned;

use crate::types::{
    article::{Article, ArticlePreview},
    category::Category,
    metadata::{ArticleMetadata, CategoryMetadata},
};

type WitCategoryMetadata = thought_plugin::CategoryMetadata;
type WitCategory = thought_plugin::Category;
type WitArticleMetadata = thought_plugin::ArticleMetadata;
type WitArticlePreview = thought_plugin::ArticlePreview;
type WitArticle = thought_plugin::Article;

impl From<Article> for WitArticle {
    fn from(value: Article) -> Self {
        WitArticle {
            preview: value.preview().clone().into(),
            content: value.content().to_owned(),
        }
    }
}

impl From<ArticlePreview> for WitArticlePreview {
    fn from(value: ArticlePreview) -> Self {
        WitArticlePreview {
            title: value.title().to_owned(),
            slug: value.slug().to_owned(),
            category: value.category().clone().into(),
            metadata: value.metadata().clone().into(),
            description: value.description().to_owned(),
        }
    }
}

impl From<Category> for WitCategory {
    fn from(value: Category) -> Self {
        WitCategory {
            path: value.path().clone(),
            metadata: value.metadata().clone().into(),
        }
    }
}

impl From<CategoryMetadata> for WitCategoryMetadata {
    fn from(value: CategoryMetadata) -> Self {
        WitCategoryMetadata {
            created: value.created().into(),
            name: value.name().to_owned(),
            description: value.description().to_owned(),
        }
    }
}

impl From<ArticleMetadata> for WitArticleMetadata {
    fn from(value: ArticleMetadata) -> Self {
        WitArticleMetadata {
            created: value.created().into(),
            tags: value.tags().to_vec(),
            author: value.author().to_owned(),
            description: value.description().map(ToOwned::to_owned),
        }
    }
}
