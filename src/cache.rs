use std::{path::PathBuf, sync::Arc};

use bincode::{self};
use color_eyre::eyre;
use redb::{Database, ReadableDatabase, TableDefinition};
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

/// Render cache backed by redb with direct database access.
///
/// This implementation queries redb directly instead of maintaining an in-memory
/// HashMap, eliminating the need for Mutex synchronization and O(N) persist operations.
#[derive(Debug, Clone)]
pub struct RenderCache {
    db: Arc<Database>,
}

impl RenderCache {
    pub async fn load(path: PathBuf) -> eyre::Result<Self> {
        let db = open_database(path).await?;
        ensure_cache_table(&db).await?;
        Ok(Self { db })
    }

    /// Check if there's a valid cache hit for the given article.
    /// Returns the cached HTML if found and valid, None otherwise.
    pub async fn hit(&self, article: &Article, theme_fingerprint: &str) -> Option<Arc<str>> {
        let key = Self::article_key(article);
        let db = Arc::clone(&self.db);
        let sha256 = article.sha256();
        let title = article.title().to_string();
        let description = article.description().to_string();
        let metadata = article.metadata().clone();
        let theme_fp = theme_fingerprint.to_string();

        spawn_blocking(move || -> Option<Arc<str>> {
            let txn = db.begin_read().ok()?;
            let table = txn.open_table(CACHE_TABLE).ok()?;
            let value = table.get(key.as_str()).ok()??;
            let cached: CachedArticle = bincode::deserialize(value.value()).ok()?;

            // Reconstruct the check inline to avoid borrowing issues
            if cached.sha256 == sha256
                && cached.title == title
                && cached.description == description
                && cached.metadata == metadata
                && cached.theme_fingerprint == theme_fp
            {
                Some(Arc::from(cached.html))
            } else {
                None
            }
        })
        .await
        .ok()?
    }

    /// Store rendered HTML for an article directly to the database.
    /// This is an incremental write - no separate persist() call needed.
    pub async fn store(
        &self,
        article: &Article,
        html: &str,
        theme_fingerprint: &str,
    ) -> eyre::Result<()> {
        let key = Self::article_key(article);
        let cached = CachedArticle::from_article(article, html, theme_fingerprint);
        let bytes = bincode::serialize(&cached)?;
        let db = Arc::clone(&self.db);

        spawn_blocking(move || -> eyre::Result<()> {
            let txn = db.begin_write()?;
            {
                let mut table = txn.open_table(CACHE_TABLE)?;
                table.insert(key.as_str(), bytes.as_slice())?;
            }
            txn.commit()?;
            Ok(())
        })
        .await?
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
