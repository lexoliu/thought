use thought_core::{
    article::ArticlePreview,
    metadata::{PluginSource, WorkspaceMetadata},
};

pub struct PluginManager {}

impl PluginManager {
    pub async fn load(plugin: &WorkspaceMetadata) -> Self {
        for (name, source) in plugin.plugins() {
            match source {
                PluginSource::CratesIo { version } => {
                    todo!("Load plugin {} from crates.io version {}", name, version);
                }
                PluginSource::Git { repo, rev } => {
                    todo!("Load plugin {} from git repo {} rev {:?}", name, repo, rev);
                }

                PluginSource::Url(url) => {
                    todo!("Load plugin {} from url {}", name, url);
                }
            }
        }
        todo!("Load plugin from workspace metadata");
    }

    // load wasm plugin from bytes
    fn register(&mut self, data: &[u8]) {
        todo!("Load wasm plugin from bytes");
    }

    #[must_use]
    pub fn render_index(
        &self,
        workspace: WorkspaceMetadata,
        articles: Vec<ArticlePreview>,
    ) -> String {
        todo!("Render index using plugins");
    }

    #[must_use]
    pub fn render_article(
        &self,
        workspace: WorkspaceMetadata,
        article: thought_core::article::Article,
    ) -> String {
        todo!("Render article using plugins");
    }
}
