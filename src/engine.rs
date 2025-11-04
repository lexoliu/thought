use anyhow::anyhow;
use std::{collections::HashMap, path::Path, sync::Arc};

use futures::future::try_join_all;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use tokio::{fs::read_to_string, spawn};
use tokio_stream::StreamExt;

use crate::{
    plugin::PluginManager,
    types::article::{Article, ArticlePreview},
    workspace::Workspace,
};

/// The engine that powers Thought
#[derive(Clone)]
pub struct Engine(Arc<EngineInner>);

struct EngineInner {
    workspace: Workspace,
    plugins: PluginManager,
}

#[derive(Serialize, Deserialize)]
struct OutputMetadata {
    #[serde(with = "time::serde::rfc3339")]
    last_generated: OffsetDateTime,
    articles: Vec<OutputArticleMetadata>,
}

#[derive(Serialize, Deserialize)]
struct OutputArticleMetadata {
    path: Vec<String>,
    sha256: String,
}

impl Engine {
    pub async fn new(workspace: Workspace) -> anyhow::Result<Self> {
        let plugins = PluginManager::from_workspace(workspace.clone())
            .await
            .map_err(|err| anyhow!(err))?;
        Ok(Self(Arc::new(EngineInner { workspace, plugins })))
    }

    #[allow(clippy::missing_panics_doc)]
    pub async fn generate(&self, output: impl AsRef<Path>) -> Result<(), std::io::Error> {
        let output = output.as_ref().to_path_buf();
        // Check `.meta.json` in output path, it contains metadata about the last generation
        let meta_path = output.join(".meta.json");
        let mut map: HashMap<Vec<String>, String> = HashMap::new();
        if meta_path.exists() {
            let content = read_to_string(meta_path).await?;
            let metadata: OutputMetadata = serde_json::from_str(&content)?;
            for article in metadata.articles {
                map.insert(article.path, article.sha256);
            }
        }

        self.0
            .plugins
            .copy_theme_assets(output.join("assets"))?;

        let mut articles_preview = Vec::new();
        let mut changed_articles = Vec::new();

        while let Some(article) = self.0.workspace.articles().next().await {
            let sha256 = article.sha256();
            let path: Vec<String> = article
                .category()
                .path()
                .clone()
                .into_iter()
                .chain([article.slug().to_string()])
                .collect();
            articles_preview.push(article.preview().clone());
            if map.get(&path) != Some(&sha256) {
                changed_articles.push(article);
            }
        }

        let mut tasks = Vec::with_capacity(changed_articles.len() + 1);

        {
            let engine = self.clone();
            let articles_preview = articles_preview.clone();
            let output = output.clone();
            tasks.push(spawn(async move {
                engine
                    .render_index(&articles_preview, output.join("index.html"))
                    .await
            }));
        }

        for article in changed_articles {
            let engine = self.clone();
            let articles_preview = articles_preview.clone();
            let output = output.clone();
            tasks.push(spawn(async move {
                engine
                    .render_article(
                        &article,
                        &articles_preview,
                        output.join(format!("{}.html", article.slug())),
                    )
                    .await
            }));
        }

        try_join_all(tasks).await?;

        Ok(())
    }

    async fn render_article(
        &self,
        article: &Article,
        _articles: &[ArticlePreview],
        output: impl AsRef<Path>,
    ) -> Result<(), std::io::Error> {
        todo!()
    }

    async fn render_index(
        &self,
        articles: &[ArticlePreview],
        output: impl AsRef<Path>,
    ) -> Result<(), std::io::Error> {
        todo!()
    }
}
