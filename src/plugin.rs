use anyhow::{Result, anyhow};
use std::{env::temp_dir, path::Path, sync::Arc};
use wasmtime::{
    Store,
    component::{Component, Instance, Linker},
};

use wasmtime_wasi::{DirPerms, FilePerms, ResourceTable, WasiCtx, WasiCtxView, WasiView};

use crate::workspace::Workspace;
use thought_core::{
    article::{Article, ArticlePreview},
    category::Category,
    metadata::{ArticleMetadata, CategoryMetadata},
};
use time::OffsetDateTime;

mod bindings {
    wasmtime::component::bindgen!({
        path: "wit",
        world: "theme-plugin",
    });
}

use bindings::ThemePlugin;

/// Manages Wasm plugins, including loading and running them.
pub struct PluginManager {
    workspace: Workspace,
    theme: Arc<ThemeRuntime>,
}

impl PluginManager {
    pub fn new(workspace: Workspace) -> Self {
        todo!()
    }

    pub fn workspace(&self) -> &Workspace {
        &self.workspace
    }

    pub fn theme_runtime(&self) -> Arc<ThemeRuntime> {
        self.theme.clone()
    }
}

/// Represents a runtime instance of a theme plugin.
///
/// Theme is a pure function, taking article data as input and producing HTML as output.
///
/// Theme runtime has no access to filesystem,time,random,or network.
struct ThemeRuntime {
    name: String,
    bindings: ThemePlugin,
    store: Store<()>,
}

impl ThemeRuntime {
    pub async fn new(name: String, binary: &[u8]) -> Result<Self> {
        let mut config = wasmtime::Config::new();
        config.async_support(true);
        let engine = wasmtime::Engine::new(&config)?;
        let component = Component::new(&engine, binary)?;
        let mut store = Store::new(&engine, ());
        let linker = Linker::new(&engine);
        let bindings = ThemePlugin::instantiate(&mut store, &component, &linker)?;
        Ok(Self {
            name,
            bindings,
            store,
        })
    }

    pub async fn generate_page(&mut self, article: &Article) -> Result<String> {
        let input = convert::article(article);
        let result = self
            .bindings
            .thought_plugin_theme()
            .call_generate_page(&mut self.store, &input)?;
        Ok(result)
    }

    pub async fn generate_index(&mut self, articles: &[ArticlePreview]) -> Result<String> {
        let input: Vec<_> = articles.iter().map(convert::article_preview).collect();
        let result = self
            .bindings
            .thought_plugin_theme()
            .call_generate_index(&mut self.store, &input)?;
        Ok(result)
    }
}

mod convert {
    use super::{
        Article, ArticleMetadata, ArticlePreview, Category, CategoryMetadata, OffsetDateTime,
        bindings,
    };
    use std::borrow::ToOwned;

    type WitTimestamp = bindings::thought::plugin::types::Timestamp;
    type WitCategoryMetadata = bindings::thought::plugin::types::CategoryMetadata;
    type WitCategory = bindings::thought::plugin::types::Category;
    type WitArticleMetadata = bindings::thought::plugin::types::ArticleMetadata;
    type WitArticlePreview = bindings::thought::plugin::types::ArticlePreview;
    type WitArticle = bindings::thought::plugin::types::Article;

    pub fn article(article: &Article) -> WitArticle {
        WitArticle {
            preview: article_preview(article.preview()),
            content: article.content().to_owned(),
        }
    }

    pub fn article_preview(preview: &ArticlePreview) -> WitArticlePreview {
        WitArticlePreview {
            title: preview.title().to_owned(),
            slug: preview.slug().to_owned(),
            category: category(preview.category()),
            metadata: article_metadata(preview.metadata()),
            description: preview.description().to_owned(),
        }
    }

    fn category(category: &Category) -> WitCategory {
        WitCategory {
            path: category.path().clone(),
            metadata: category_metadata(category.metadata()),
        }
    }

    fn category_metadata(metadata: &CategoryMetadata) -> WitCategoryMetadata {
        WitCategoryMetadata {
            created: timestamp(metadata.created()),
            name: metadata.name().to_owned(),
            description: metadata.description().to_owned(),
        }
    }

    fn article_metadata(metadata: &ArticleMetadata) -> WitArticleMetadata {
        WitArticleMetadata {
            created: timestamp(metadata.created()),
            tags: metadata.tags().to_vec(),
            author: metadata.author().to_owned(),
            description: metadata.description().map(ToOwned::to_owned),
        }
    }

    fn timestamp(value: OffsetDateTime) -> WitTimestamp {
        WitTimestamp {
            seconds: value.unix_timestamp(),
            nanos: value.nanosecond(),
        }
    }
}

/// Represents a runtime instance of a plugin.
///
/// Plugins operate with restricted capabilities for security and isolation:
///
/// ## Capabilities
/// - **Random**: Access to random number generation
/// - **Time**: Access to system time
///
/// ## Filesystem
/// Each plugin has an isolated virtual filesystem:
/// - `/tmp` - Read/write temporary storage (/tmp will be cleared on each run)
/// - `/cache` - Read/write cache storage (we will make efforts to persist cache between runs)
/// - `/build` - Read/write build artifacts
/// - `/assets` - Read-only bundled resources
///
/// ## Network
/// Network access is not supported by now. It is planned to be added in the future.
pub struct PluginRuntime {
    name: String,
    store: Store<PluginRuntimeState>,
    _instance: Instance,
}

struct PluginRuntimeState {
    wasi: WasiCtx,
    table: ResourceTable,
}

impl PluginRuntimeState {
    pub fn new(
        name: &str,
        cache_path: &Path,
        assets_path: &Path,
        build_path: Option<&Path>,
    ) -> Result<Self> {
        let mut wasi = WasiCtx::builder();

        let tmp_dir = temp_dir().join("thought-plugins").join(name);
        let cache_dir = cache_path.join("thought-plugins").join(name);

        std::fs::create_dir_all(&tmp_dir)?;
        std::fs::create_dir_all(&cache_dir)?;

        wasi.preopened_dir(&tmp_dir, "/tmp", DirPerms::all(), FilePerms::all())?;
        wasi.preopened_dir(&cache_dir, "/cache", DirPerms::all(), FilePerms::all())?;
        wasi.preopened_dir(assets_path, "/assets", DirPerms::READ, FilePerms::READ)?;

        if let Some(build_path) = build_path {
            std::fs::create_dir_all(build_path)?;
            wasi.preopened_dir(build_path, "/build", DirPerms::all(), FilePerms::all())?;
        }

        Ok(Self {
            wasi: wasi.build(),
            table: ResourceTable::new(),
        })
    }
}

impl WasiView for PluginRuntimeState {
    fn ctx(&mut self) -> wasmtime_wasi::WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi,
            table: &mut self.table,
        }
    }
}

impl PluginRuntime {
    pub async fn new(
        name: String,
        binary: &[u8],
        cache_path: &Path,
        assets_path: &Path,
        build_path: Option<&Path>,
    ) -> Result<Self> {
        let mut config = wasmtime::Config::new();
        config.async_support(true);
        let engine = wasmtime::Engine::new(&config)?;

        let state = PluginRuntimeState::new(&name, cache_path, assets_path, build_path)?;
        let component = Component::new(&engine, binary)?;
        let mut linker: Linker<PluginRuntimeState> = Linker::new(&engine);
        wasmtime_wasi::p2::add_to_linker_async(&mut linker)?;

        let mut store = Store::new(&engine, state);
        let instance = linker.instantiate_async(&mut store, &component).await?;

        Ok(Self {
            name,
            store,
            _instance: instance,
        })
    }
}
