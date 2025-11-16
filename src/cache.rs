use std::{collections::HashMap, io::ErrorKind, path::PathBuf};

use bincode::{self};
use color_eyre::eyre;
use serde::{Deserialize, Serialize};
use tokio::fs;

use crate::{article::Article, metadata::ArticleMetadata};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedArticle {
    sha256: String,
    title: String,
    description: String,
    metadata: ArticleMetadata,
    html: String,
}

impl CachedArticle {
    fn matches(&self, article: &Article) -> bool {
        self.sha256 == article.sha256()
            && self.title == article.title()
            && self.description == article.description()
            && self.metadata == *article.metadata()
    }

    fn from_article(article: &Article, html: &str) -> Self {
        Self {
            sha256: article.sha256(),
            title: article.title().to_string(),
            description: article.description().to_string(),
            metadata: article.metadata().clone(),
            html: html.to_string(),
        }
    }
}

#[derive(Debug)]
pub struct RenderCache {
    entries: HashMap<String, CachedArticle>,
    path: PathBuf,
}

impl RenderCache {
    pub async fn load(path: PathBuf) -> eyre::Result<Self> {
        let entries = match fs::read(&path).await {
            Ok(bytes) => {
                bincode::deserialize::<HashMap<String, CachedArticle>>(&bytes).unwrap_or_default()
            }
            Err(err) if err.kind() == ErrorKind::NotFound => HashMap::new(),
            Err(err) => return Err(err.into()),
        };
        Ok(Self { entries, path })
    }

    pub fn hit(&self, article: &Article) -> Option<String> {
        let key = Self::article_key(article);
        self.entries
            .get(&key)
            .and_then(|entry| entry.matches(article).then(|| entry.html.clone()))
    }

    pub fn store(&mut self, article: &Article, html: &str) {
        let key = Self::article_key(article);
        self.entries
            .insert(key, CachedArticle::from_article(article, html));
    }

    pub async fn persist(&self) -> eyre::Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let bytes = bincode::serialize(&self.entries)?;
        fs::write(&self.path, bytes).await?;
        Ok(())
    }

    fn article_key(article: &Article) -> String {
        article.segments().join("/")
    }
}
