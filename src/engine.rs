use std::path::Path;

use color_eyre::eyre;
use futures::TryStreamExt;
use tokio::fs as async_fs;

use crate::{
    article::Article,
    plugin::{IndexToken, PluginManager},
    utils::write,
    workspace::Workspace,
};

pub struct Engine {
    workspace: Workspace,
    plugins: PluginManager,
}

impl Engine {
    pub async fn new(workspace: Workspace) -> eyre::Result<Self> {
        let plugins = PluginManager::resolve_workspace(&workspace).await?;
        Ok(Self { workspace, plugins })
    }

    pub async fn generate(&self, output: impl AsRef<Path>) -> eyre::Result<()> {
        let output = output.as_ref();
        if async_fs::metadata(output).await.is_ok() {
            async_fs::remove_dir_all(output).await?;
        }
        async_fs::create_dir_all(output).await?;

        let mut tokens: Vec<IndexToken> = Vec::new();
        let stream = self.workspace.articles();
        futures::pin_mut!(stream);

        while let Some(article) = stream.try_next().await? {
            let rendered = self.plugins.render_article(&article)?;
            let (token, html) = rendered.into_parts();
            tokens.push(token);
            write_article(output, &article, &html).await?;
        }

        let index_html = self.plugins.render_index(&tokens)?;
        write(output.join("index.html"), index_html.as_bytes()).await?;

        Ok(())
    }
}

async fn write_article(output: &Path, article: &Article, html: &str) -> std::io::Result<()> {
    let mut dir = output.to_path_buf();
    for segment in article.segments() {
        dir.push(segment);
    }
    async_fs::create_dir_all(&dir).await?;
    write(dir.join("index.html"), html.as_bytes()).await
}
