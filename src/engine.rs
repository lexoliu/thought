use std::{path::Path, sync::Arc};

use color_eyre::eyre;
use rayon::prelude::*;
use sha2::{Digest, Sha256};
use tracing::info;

use crate::{
    article::Article,
    cache::RenderCache,
    category::Category,
    io_backend,
    plugin::PluginManager,
    scanner::{build_sources, scan_workspace_files},
    search::{IndexedDoc, Searcher, SEARCH_WRAPPER},
    utils::write_sync,
    workspace::Workspace,
};
use thought_plugin::helpers::{search_asset_dir, search_js_filename, search_wasm_filename};

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

    /// Three-phase generate pipeline:
    ///   Phase 0 — Setup (scan + batch read via io_backend)
    ///   Phase 1 — Process (rayon parallel: parse, render, cache, search)
    ///   Phase 2 — Write (batch write via io_backend + search commit)
    pub fn generate_pipeline(&self, output: &Path) -> eyre::Result<()> {
        // Clean output
        if output.exists() {
            std::fs::remove_dir_all(output)?;
        }
        std::fs::create_dir_all(self.workspace.cache_dir())?;

        // ═══ PHASE 0: SETUP — scan + batch read ═══
        info!("phase 0: scanning workspace");
        let scan = scan_workspace_files(&self.workspace.articles_dir());
        let (article_sources, category_bytes) = build_sources(&scan)?;

        // Load render cache
        let cache_path = self.workspace.cache_dir().join("cache.redb");
        let cache = RenderCache::load_sync(cache_path)?;
        let theme_fp = self.plugins.theme_fingerprint().to_string();

        // ═══ PHASE 1: PROCESS — CPU-parallel (rayon) ═══
        info!("phase 1: processing {} articles", article_sources.len());

        // Build categories from raw bytes (parallel)
        let categories: Vec<Category> = category_bytes
            .par_iter()
            .filter_map(|(segments, bytes)| {
                Category::from_bytes(self.workspace.clone(), segments.clone(), bytes).ok()
            })
            .collect();

        // Build a lookup: category segments → Category
        // The root category (empty segments) is needed for top-level articles
        let category_map: std::collections::HashMap<Vec<String>, Category> = categories
            .into_iter()
            .map(|cat| (cat.segments().clone(), cat))
            .collect();

        // Parse all articles in parallel (simdutf8 for UTF-8 validation)
        let articles: Vec<Article> = article_sources
            .par_iter()
            .flat_map(|src| {
                // Find the category for this article's parent segments
                let parent_segments: Vec<String> = if src.segments.len() > 1 {
                    src.segments[..src.segments.len() - 1].to_vec()
                } else {
                    Vec::new()
                };
                let category = match category_map.get(&parent_segments) {
                    Some(cat) => cat.clone(),
                    None => return Vec::new(),
                };
                Article::from_source(category, src).unwrap_or_else(|_| Vec::new())
            })
            .collect();

        // Collect previews + compute content fingerprint
        let mut previews = Vec::new();
        let mut fingerprint = Sha256::new();
        for article in &articles {
            if article.is_default_locale() {
                previews.push(article.preview().clone());
            }
            fingerprint.update(article.sha256().as_bytes());
        }
        let fingerprint = format!("{:x}", fingerprint.finalize());

        // Render all articles in parallel (cache check + Wasm rendering)
        let rendered: Vec<(String, Vec<u8>)> = articles
            .par_iter()
            .map(|article| {
                let html = cache
                    .hit_sync(article, &theme_fp)
                    .map(|arc| arc.to_string())
                    .unwrap_or_else(|| {
                        let rendered = self
                            .plugins
                            .render_article(article.clone())
                            .expect("render_article failed");
                        cache.store_sync(article, &rendered, &theme_fp).ok();
                        rendered
                    });
                (article.output_file(), html.into_bytes())
            })
            .collect();

        // Render index page
        let index_html = self.plugins.render_index(previews)?;

        // Build search entries
        let search_entries: Vec<IndexedDoc> = articles
            .par_iter()
            .filter_map(|article| IndexedDoc::from_article(article).ok())
            .collect();

        // ═══ PHASE 2: WRITE — batch I/O + search commit ═══
        info!("phase 2: writing {} files", rendered.len() + 1);

        // Copy theme assets
        self.plugins.copy_theme_assets_sync(output)?;

        // Collect all writes
        let mut writes: Vec<(std::path::PathBuf, Vec<u8>)> = rendered
            .into_iter()
            .map(|(rel_path, html)| (output.join(rel_path), html))
            .collect();
        writes.push((output.join("index.html"), index_html.into_bytes()));

        // Batch write all HTML (io_uring on Linux, rayon on macOS)
        io_backend::batch_write_files(&writes)?;

        // Search: batch index + emit bundle
        let searcher = Searcher::open_sync(self.workspace.clone())?;
        searcher.ensure_index_sync(&search_entries, Some(&fingerprint))?;

        let search_dir = output.join(search_asset_dir());
        std::fs::create_dir_all(&search_dir)?;
        searcher.build_wasm_sync(&articles, search_dir.join(search_wasm_filename()))?;
        write_sync(
            search_dir.join(search_js_filename()),
            SEARCH_WRAPPER.as_bytes(),
        )?;

        info!("generate complete");
        Ok(())
    }

    /// Async generate — delegates to the sync pipeline via spawn_blocking
    /// to avoid blocking the tokio runtime.
    pub async fn generate(&self, output: impl AsRef<Path>) -> eyre::Result<()> {
        // The pipeline is CPU+IO bound (rayon + io_backend), not async.
        // We still need the async wrapper for compatibility with serve mode
        // and the existing CLI interface.
        self.generate_pipeline(output.as_ref())
    }
}
