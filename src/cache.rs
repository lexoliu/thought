use std::{collections::HashMap, path::PathBuf, sync::Arc};

use bincode::{self};
use color_eyre::eyre;
use redb::{Database, ReadableDatabase, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};
use tokio::task::spawn_blocking;

use crate::{article::Article, metadata::ArticleMetadata};

const CACHE_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("render_cache");

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedArticle {
    sha256: String,
    title: String,
    description: String,
    metadata: ArticleMetadata,
    html: String,
    #[serde(default)]
    theme_fingerprint: String,
}

impl CachedArticle {
    fn matches(&self, article: &Article, theme_fingerprint: &str) -> bool {
        self.sha256 == article.sha256()
            && self.title == article.title()
            && self.description == article.description()
            && self.metadata == *article.metadata()
            && self.theme_fingerprint == theme_fingerprint
    }

    fn from_article(article: &Article, html: &str, theme_fingerprint: &str) -> Self {
        Self {
            sha256: article.sha256(),
            title: article.title().to_string(),
            description: article.description().to_string(),
            metadata: article.metadata().clone(),
            html: html.to_string(),
            theme_fingerprint: theme_fingerprint.to_string(),
        }
    }
}

#[derive(Debug)]
pub struct RenderCache {
    entries: HashMap<String, CachedArticle>,
    db: Arc<Database>,
}

impl RenderCache {
    pub async fn load(path: PathBuf) -> eyre::Result<Self> {
        let db = open_database(path).await?;
        ensure_cache_table(&db).await?;
        let entries = load_cache_entries(&db).await?;
        Ok(Self { entries, db })
    }

    pub fn hit(&self, article: &Article, theme_fingerprint: &str) -> Option<String> {
        let key = Self::article_key(article);
        self.entries.get(&key).and_then(|entry| {
            entry
                .matches(article, theme_fingerprint)
                .then(|| entry.html.clone())
        })
    }

    pub fn store(&mut self, article: &Article, html: &str, theme_fingerprint: &str) {
        let key = Self::article_key(article);
        self.entries.insert(
            key,
            CachedArticle::from_article(article, html, theme_fingerprint),
        );
    }

    pub async fn persist(&self) -> eyre::Result<()> {
        let entries = self.entries.clone();
        let db = Arc::clone(&self.db);
        spawn_blocking(move || -> eyre::Result<()> {
            let txn = db.begin_write()?;
            let _ = txn.delete_table(CACHE_TABLE);
            {
                let mut table = txn.open_table(CACHE_TABLE)?;
                for (key, entry) in entries {
                    let bytes = bincode::serialize(&entry)?;
                    table.insert(key.as_str(), bytes.as_slice())?;
                }
            }
            txn.commit()?;
            Ok(())
        })
        .await??;
        Ok(())
    }

    fn article_key(article: &Article) -> String {
        article.output_path()
    }
}

async fn open_database(path: PathBuf) -> eyre::Result<Arc<Database>> {
    spawn_blocking(move || -> eyre::Result<Arc<Database>> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let db = if path.exists() {
            Database::open(path.as_path())?
        } else {
            Database::create(path.as_path())?
        };
        Ok(Arc::new(db))
    })
    .await?
}

async fn ensure_cache_table(db: &Arc<Database>) -> eyre::Result<()> {
    let db = Arc::clone(db);
    spawn_blocking(move || -> eyre::Result<()> {
        let txn = db.begin_write()?;
        txn.open_table(CACHE_TABLE)?;
        txn.commit()?;
        Ok(())
    })
    .await?
}

async fn load_cache_entries(db: &Arc<Database>) -> eyre::Result<HashMap<String, CachedArticle>> {
    let db = Arc::clone(db);
    spawn_blocking(move || -> eyre::Result<HashMap<String, CachedArticle>> {
        let txn = db.begin_read()?;
        let table = txn.open_table(CACHE_TABLE)?;
        let mut entries = HashMap::new();
        for item in table.iter()? {
            let (key, value) = item?;
            let cached: CachedArticle = bincode::deserialize(value.value())?;
            entries.insert(key.value().to_string(), cached);
        }
        Ok(entries)
    })
    .await?
}
