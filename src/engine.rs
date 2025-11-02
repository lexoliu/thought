use std::{collections::HashMap, path::Path, sync::Arc};

use serde::{Deserialize, Serialize};
use smol::{Task, fs::read_to_string, spawn, stream::StreamExt};
use thought_core::article::{self, ArticlePreview};
use time::OffsetDateTime;

use crate::{plugin::PluginManager, workspace::Workspace};

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
        let plugins = PluginManager::load(&workspace).await?;
        let inner = EngineInner { workspace, plugins };
        Ok(Self(Arc::new(inner)))
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

        let mut render_task: Vec<Task<Result<(), std::io::Error>>> = Vec::new();

        // create rendering index task

        let index_task = {
            let articles_preview = articles_preview.clone();
            let output = output.clone();
            let engine = self.clone();
            spawn(async move {
                engine
                    .render_index(&articles_preview, output.join("index.html"))
                    .await
            })
        };
        render_task.push(index_task);

        for article in changed_articles {
            let articles_preview = articles_preview.clone();
            let output = output.clone();
            let engine = self.clone();
            render_task.push(spawn(async move {
                engine
                    .render_article(
                        &article,
                        &articles_preview,
                        output.join(format!("{}.html", article.slug())),
                    )
                    .await
            }));
        }

        // wait for all render task to finish
        for task in render_task {
            task.await?;
        }

        Ok(())
    }

    async fn render_article(
        &self,
        article: &article::Article,
        _articles: &[ArticlePreview],
        output: impl AsRef<Path>,
    ) -> Result<(), std::io::Error> {
        let content = self
            .0
            .plugins
            .render_article(self.0.workspace.metadata().clone(), article.clone())
            .await
            .map_err(std::io::Error::other)?;
        smol::fs::write(output, content).await
    }

    async fn render_index(
        &self,
        articles: &[ArticlePreview],
        output: impl AsRef<Path>,
    ) -> Result<(), std::io::Error> {
        let content = self
            .0
            .plugins
            .render_index(self.0.workspace.metadata().clone(), articles.to_vec())
            .await
            .map_err(std::io::Error::other)?;
        smol::fs::write(output, content).await
    }
}
