use std::{path::Path, sync::Arc};

use color_eyre::eyre;
use futures::TryStreamExt;
use tokio::{fs as async_fs, spawn, task::JoinHandle};

use crate::{plugin::PluginManager, utils::write, workspace::Workspace};

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

        let stream = self.workspace.articles();
        futures::pin_mut!(stream);

        let mut tasks: Vec<JoinHandle<eyre::Result<()>>> = Vec::new();

        let mut previews = Vec::new();

        while let Some(article) = stream.try_next().await? {
            let plugins = self.plugins.clone();
            previews.push(article.preview().clone());
            let relative_path = article.segments().join("/");
            let article_output = output.join(format!("{relative_path}.html"));

            tasks.push(spawn(async move {
                let rendered = plugins.render_article(article)?;
                write(article_output, rendered.as_bytes()).await?;
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

        Ok(())
    }
}
