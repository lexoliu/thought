use anyhow::Result;
use std::{env::temp_dir, path::Path, sync::Arc};
use tokio::sync::Mutex;
use wasmtime::{
    Config, Engine, Store,
    component::{Component, Linker},
};

use wasmtime_wasi::{DirPerms, FilePerms, ResourceTable, WasiCtx, WasiCtxView, WasiView};

use crate::types::article::{Article, ArticlePreview};
use crate::workspace::Workspace;

use bindings::{
    plugin::LifecycleRuntime as PluginComponent, theme::ThemeRuntime as ThemeComponent,
};

mod bindings {
    pub mod plugin {
        wasmtime::component::bindgen!({
            path: "plugin/wit/plugin.wit",
            world: "hook-runtime",
        });

        pub type LifecycleRuntime = HookRuntime;
    }

    pub mod theme {
        wasmtime::component::bindgen!({
            path: "plugin/wit/plugin.wit",
            world: "theme-runtime",
        });
    }
}

/// Orchestrates deterministic theme rendering with sequential lifecycle plugins.
pub struct PluginManager {
    workspace: Workspace,
    theme: Arc<ThemeRuntime>,
    plugins: Vec<Arc<Mutex<PluginRuntime>>>,
}

impl PluginManager {
    pub fn new(workspace: Workspace, theme: ThemeRuntime, plugins: Vec<PluginRuntime>) -> Self {
        Self {
            workspace,
            theme: Arc::new(theme),
            plugins: plugins
                .into_iter()
                .map(|p| Arc::new(Mutex::new(p)))
                .collect(),
        }
    }

    pub fn from_workspace(workspace: Workspace) -> Result<Self> {
        let name = workspace.metadata().theme().name().to_owned();
        let message =
            format!("loading theme `{name}` directly from workspace is not implemented yet");
        Err(anyhow::Error::msg(message))
    }

    pub fn workspace(&self) -> &Workspace {
        &self.workspace
    }

    pub fn theme_runtime(&self) -> Arc<ThemeRuntime> {
        self.theme.clone()
    }

    pub fn plugins(&self) -> impl Iterator<Item = Arc<Mutex<PluginRuntime>>> + '_ {
        self.plugins.iter().cloned()
    }

    /// Apply the `on_pre_render` lifecycle hook of each plugin in declaration order.
    pub async fn apply_pre_render(&self, article: Article) -> Result<Article> {
        let mut current = article;
        for runtime in &self.plugins {
            let mut plugin = runtime.lock().await;
            current = plugin.on_pre_render(&current)?;
        }
        Ok(current)
    }

    /// Apply the `on_post_render` hook sequentially, letting each plugin transform the HTML.
    pub async fn apply_post_render(&self, article: &Article, html: String) -> Result<String> {
        let mut current = html;
        for runtime in &self.plugins {
            let mut plugin = runtime.lock().await;
            current = plugin.on_post_render(article, &current)?;
        }
        Ok(current)
    }
}

/// Represents a runtime instance of a theme plugin.
///
/// Theme is a pure function, taking article data as input and producing HTML as output.
///
/// Theme runtime has no access to filesystem, time, randomness, or network interfaces.
/// The host instantiates the component per render to keep evaluation pure and parallel-safe.
#[derive(Clone)]
pub struct ThemeRuntime {
    name: String,
    engine: Engine,
    component: Component,
}

impl ThemeRuntime {
    pub fn new(name: impl Into<String>, binary: &[u8]) -> Result<Self> {
        let mut config = Config::new();
        config.async_support(false);
        let engine = Engine::new(&config)?;
        let component = Component::new(&engine, binary)?;
        Ok(Self {
            name: name.into(),
            engine,
            component,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    fn instantiate(&self) -> Result<(Store<()>, ThemeComponent)> {
        let mut store = Store::new(&self.engine, ());
        let linker = Linker::new(&self.engine);
        let bindings = ThemeComponent::instantiate(&mut store, &self.component, &linker)?;
        Ok((store, bindings))
    }

    pub fn generate_page(&self, article: &Article) -> Result<String> {
        let input = convert::theme::article(article);
        let (mut store, bindings) = self.instantiate()?;
        let result = bindings
            .thought_plugin_theme()
            .call_generate_page(&mut store, &input)?;
        Ok(result)
    }

    pub fn generate_index(&self, articles: &[ArticlePreview]) -> Result<String> {
        let input: Vec<_> = articles
            .iter()
            .map(convert::theme::article_preview)
            .collect();
        let (mut store, bindings) = self.instantiate()?;
        let result = bindings
            .thought_plugin_theme()
            .call_generate_index(&mut store, &input)?;
        Ok(result)
    }
}

mod convert {
    use super::bindings;
    use crate::types::{
        article::{Article, ArticlePreview},
        category::Category,
        metadata::{ArticleMetadata, CategoryMetadata},
    };
    use std::borrow::ToOwned;
    use time::{Duration, OffsetDateTime};

    fn to_timestamp_parts(value: OffsetDateTime) -> (i64, u32) {
        (value.unix_timestamp(), value.nanosecond())
    }

    fn from_timestamp_parts((seconds, nanos): (i64, u32)) -> OffsetDateTime {
        OffsetDateTime::from_unix_timestamp(seconds).unwrap_or(OffsetDateTime::UNIX_EPOCH)
            + Duration::nanoseconds(i64::from(nanos))
    }

    trait IntoThemeTimestamp {
        fn into_theme_timestamp(self) -> bindings::theme::thought::plugin::types::Timestamp;
    }

    trait IntoPluginTimestamp {
        fn into_plugin_timestamp(self) -> bindings::plugin::thought::plugin::types::Timestamp;
    }

    impl IntoThemeTimestamp for OffsetDateTime {
        fn into_theme_timestamp(self) -> bindings::theme::thought::plugin::types::Timestamp {
            let (seconds, nanos) = to_timestamp_parts(self);
            bindings::theme::thought::plugin::types::Timestamp { seconds, nanos }
        }
    }

    impl IntoPluginTimestamp for OffsetDateTime {
        fn into_plugin_timestamp(self) -> bindings::plugin::thought::plugin::types::Timestamp {
            let (seconds, nanos) = to_timestamp_parts(self);
            bindings::plugin::thought::plugin::types::Timestamp { seconds, nanos }
        }
    }

    impl From<bindings::plugin::thought::plugin::types::Timestamp> for OffsetDateTime {
        fn from(value: bindings::plugin::thought::plugin::types::Timestamp) -> Self {
            from_timestamp_parts((value.seconds, value.nanos))
        }
    }

    impl From<&CategoryMetadata> for bindings::theme::thought::plugin::types::CategoryMetadata {
        fn from(value: &CategoryMetadata) -> Self {
            Self {
                created: value.created().into_theme_timestamp(),
                name: value.name().to_owned(),
                description: value.description().to_owned(),
            }
        }
    }

    impl From<&CategoryMetadata> for bindings::plugin::thought::plugin::types::CategoryMetadata {
        fn from(value: &CategoryMetadata) -> Self {
            Self {
                created: value.created().into_plugin_timestamp(),
                name: value.name().to_owned(),
                description: value.description().to_owned(),
            }
        }
    }

    impl From<bindings::plugin::thought::plugin::types::CategoryMetadata> for CategoryMetadata {
        fn from(value: bindings::plugin::thought::plugin::types::CategoryMetadata) -> Self {
            CategoryMetadata::from_raw(value.created.into(), value.name, value.description)
        }
    }

    impl From<&Category> for bindings::theme::thought::plugin::types::Category {
        fn from(value: &Category) -> Self {
            Self {
                path: value.path().clone(),
                metadata: value.metadata().into(),
            }
        }
    }

    impl From<&Category> for bindings::plugin::thought::plugin::types::Category {
        fn from(value: &Category) -> Self {
            Self {
                path: value.path().clone(),
                metadata: value.metadata().into(),
            }
        }
    }

    impl From<bindings::plugin::thought::plugin::types::Category> for Category {
        fn from(value: bindings::plugin::thought::plugin::types::Category) -> Self {
            Self::new(value.path, value.metadata.into())
        }
    }

    impl From<&ArticleMetadata> for bindings::theme::thought::plugin::types::ArticleMetadata {
        fn from(value: &ArticleMetadata) -> Self {
            Self {
                created: value.created().into_theme_timestamp(),
                tags: value.tags().to_vec(),
                author: value.author().to_owned(),
                description: value.description().map(ToOwned::to_owned),
            }
        }
    }

    impl From<&ArticleMetadata> for bindings::plugin::thought::plugin::types::ArticleMetadata {
        fn from(value: &ArticleMetadata) -> Self {
            Self {
                created: value.created().into_plugin_timestamp(),
                tags: value.tags().to_vec(),
                author: value.author().to_owned(),
                description: value.description().map(ToOwned::to_owned),
            }
        }
    }

    impl From<bindings::plugin::thought::plugin::types::ArticleMetadata> for ArticleMetadata {
        fn from(value: bindings::plugin::thought::plugin::types::ArticleMetadata) -> Self {
            ArticleMetadata::from_raw(
                value.created.into(),
                value.tags,
                value.author,
                value.description,
            )
        }
    }

    impl From<&ArticlePreview> for bindings::theme::thought::plugin::types::ArticlePreview {
        fn from(value: &ArticlePreview) -> Self {
            Self {
                title: value.title().to_owned(),
                slug: value.slug().to_owned(),
                category: value.category().into(),
                metadata: value.metadata().into(),
                description: value.description().to_owned(),
            }
        }
    }

    impl From<&ArticlePreview> for bindings::plugin::thought::plugin::types::ArticlePreview {
        fn from(value: &ArticlePreview) -> Self {
            Self {
                title: value.title().to_owned(),
                slug: value.slug().to_owned(),
                category: value.category().into(),
                metadata: value.metadata().into(),
                description: value.description().to_owned(),
            }
        }
    }

    impl From<&Article> for bindings::theme::thought::plugin::types::Article {
        fn from(value: &Article) -> Self {
            Self {
                preview: value.preview().into(),
                content: value.content().to_owned(),
            }
        }
    }

    impl From<&Article> for bindings::plugin::thought::plugin::types::Article {
        fn from(value: &Article) -> Self {
            Self {
                preview: value.preview().into(),
                content: value.content().to_owned(),
            }
        }
    }

    impl From<bindings::plugin::thought::plugin::types::Article> for Article {
        fn from(value: bindings::plugin::thought::plugin::types::Article) -> Self {
            let category: Category = value.preview.category.into();
            let metadata: ArticleMetadata = value.preview.metadata.into();
            Article::new(
                value.preview.title,
                value.preview.slug,
                category,
                metadata,
                value.preview.description,
                value.content,
            )
        }
    }

    pub mod theme {
        use super::*;

        pub fn article(article: &Article) -> bindings::theme::thought::plugin::types::Article {
            article.into()
        }

        pub fn article_preview(
            preview: &ArticlePreview,
        ) -> bindings::theme::thought::plugin::types::ArticlePreview {
            preview.into()
        }
    }

    pub mod plugin {
        use super::*;

        pub fn article(article: &Article) -> bindings::plugin::thought::plugin::types::Article {
            article.into()
        }

        pub fn into_article(value: bindings::plugin::thought::plugin::types::Article) -> Article {
            value.into()
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
///
/// Plugin runtimes are executed sequentially; the output of one lifecycle hook becomes the input of the next.
pub struct PluginRuntime {
    name: String,
    store: Store<PluginRuntimeState>,
    bindings: PluginComponent,
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
        let mut config = Config::new();
        config.async_support(true);
        let engine = Engine::new(&config)?;

        let state = PluginRuntimeState::new(&name, cache_path, assets_path, build_path)?;
        let component = Component::new(&engine, binary)?;
        let mut linker: Linker<PluginRuntimeState> = Linker::new(&engine);
        wasmtime_wasi::p2::add_to_linker_async(&mut linker)?;

        let mut store = Store::new(&engine, state);
        let bindings = PluginComponent::instantiate_async(&mut store, &component, &linker).await?;

        Ok(Self {
            name,
            store,
            bindings,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn on_pre_render(&mut self, article: &Article) -> Result<Article> {
        let input = convert::plugin::article(article);
        let result = self
            .bindings
            .thought_plugin_hook()
            .call_on_pre_render(&mut self.store, &input)?;
        Ok(convert::plugin::into_article(result))
    }

    pub fn on_post_render(&mut self, article: &Article, html: &str) -> Result<String> {
        let input = convert::plugin::article(article);
        let result = self
            .bindings
            .thought_plugin_hook()
            .call_on_post_render(&mut self.store, &input, html)?;
        Ok(result)
    }
}
