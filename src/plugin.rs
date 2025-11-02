use thought_core::{article::ArticlePreview, metadata::WorkspaceMetadata};

pub struct PluginManager {}

impl PluginManager {
    pub fn new() -> Self {
        Self {}
    }

    // load wasm plugin from bytes
    pub fn load(&mut self, data: &[u8]) {
        todo!("Load wasm plugin from bytes");
    }

    pub fn render_index(
        &self,
        workspace: WorkspaceMetadata,
        articles: Vec<ArticlePreview>,
    ) -> String {
        todo!("Render index using plugins");
    }

    pub fn render_article(
        &self,
        workspace: WorkspaceMetadata,
        article: thought_core::article::Article,
    ) -> String {
        todo!("Render article using plugins");
    }
}
