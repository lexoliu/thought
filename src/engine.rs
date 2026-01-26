use std::{path::Path, sync::Arc};

use color_eyre::eyre;
use futures::TryStreamExt;
use sha2::{Digest, Sha256};
use tokio::{fs as async_fs, spawn, task::JoinHandle};

use crate::{
    cache::RenderCache, plugin::PluginManager, search, utils::write, workspace::Workspace,
};

pub struct Engine {
    workspace: Workspace,
    plugins: Arc<PluginManager>,
}

impl Engine {
    pub async fn new(workspace: Workspace) -> eyre::Result<Self> {
        let plugins = PluginManager::resolve_workspace(&workspace).await?;
        Ok(Self {
            workspace,
            plugins: Arc::new(plugins),
        })
    }

    pub async fn generate(&self, output: impl AsRef<Path>) -> eyre::Result<()> {
        let output = output.as_ref();
        if async_fs::metadata(output).await.is_ok() {
            async_fs::remove_dir_all(output).await?;
        }

        async_fs::create_dir_all(self.workspace.cache_dir()).await?;
        let cache_path = self.workspace.cache_dir().join("cache.redb");
        let cache = RenderCache::load(cache_path).await?;
        let cache = Arc::new(cache); // No Mutex needed - redb handles concurrency
        self.plugins.copy_theme_assets(output).await?;

        let stream = self.workspace.articles();
        futures::pin_mut!(stream);

        let mut tasks: Vec<JoinHandle<eyre::Result<()>>> = Vec::new();

        let mut previews = Vec::new();
        let mut fingerprint = Sha256::new();
        let theme_fp = self.plugins.theme_fingerprint().to_string();

        while let Some(article) = stream.try_next().await? {
            let plugins = self.plugins.clone();
            let cache = cache.clone();
            let theme_fp = theme_fp.clone();
            if article.is_default_locale() {
                previews.push(article.preview().clone());
            }
            fingerprint.update(article.sha256().as_bytes());
            let article_output = output.join(article.output_file());

            tasks.push(spawn(async move {
                let cached_html = cache.hit(&article, &theme_fp).await;

                if let Some(html) = cached_html {
                    write(article_output, html.as_bytes()).await?;
                    return Ok(());
                }

                let rendered = plugins.render_article(article.clone())?;
                write(article_output, rendered.as_bytes()).await?;
                cache.store(&article, &rendered, &theme_fp).await?;
                Ok(())
            }));
        }

        let plugins = self.plugins.clone();
        let index_file_path = output.join("index.html");
        tasks.push(spawn(async move {
            let index_html = plugins.render_index(previews)?;
            write(index_file_path, index_html.as_bytes()).await?;
            Ok(())
        }));

        // Wait for all tasks to complete
        for task in tasks {
            task.await??;
        }

        // No persist needed - writes are incremental
        let fingerprint = format!("{:x}", fingerprint.finalize());
        search::emit_search_bundle(&self.workspace, output, Some(&fingerprint)).await?;

        Ok(())
    }
}
