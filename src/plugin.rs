use color_eyre::eyre::{self, eyre};
use wasmtime::{
    Config, Engine as WasmEngine, Store,
    component::{Component, Linker},
};
use wasmtime_wasi::{self, ResourceTable, WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};

mod bindings;
mod resolver;

use crate::{
    article::{Article, ArticlePreview},
    metadata::PluginKind,
    workspace::Workspace,
};

use bindings::{
    WITArticle, WITArticlePreview,
    hook::{self},
    theme::{self},
};
use resolver::resolve_plugin;

pub struct PluginManager {
    engine: WasmEngine,
    theme: ThemeHandle,
    hooks: Vec<HookHandle>,
}

struct ThemeHandle {
    pre: theme::ThemeRuntimePre<PluginInstanceState>,
}

struct HookHandle {
    pre: hook::HookRuntimePre<PluginInstanceState>,
}

impl PluginManager {
    pub async fn resolve_workspace(workspace: &Workspace) -> eyre::Result<Self> {
        let engine = build_engine()?;
        let mut theme = None;
        let mut hooks = Vec::new();

        for (name, locator) in workspace.manifest().plugins() {
            let mut resolved = resolve_plugin(workspace, name, locator)
                .await
                .map_err(|err: resolver::ResolvePluginError| eyre!(err))?;
            resolved.build().await?;
            let kind = resolved.manifest().kind.clone();
            let component =
                Component::from_file(&engine, resolved.wasm_path()).map_err(|err| eyre!(err))?;
            let pre = instantiate_pre(&engine, &component)?;
            match kind {
                PluginKind::Theme => {
                    let theme_pre = theme::ThemeRuntimePre::new(pre)
                        .map_err(|err: wasmtime::Error| eyre!(err))?;
                    theme = Some(ThemeHandle { pre: theme_pre });
                }
                PluginKind::Hook => {
                    let hook_pre = hook::HookRuntimePre::new(pre)
                        .map_err(|err: wasmtime::Error| eyre!(err))?;
                    hooks.push(HookHandle { pre: hook_pre });
                }
            }
        }

        let theme = theme.ok_or_else(|| {
            eyre!(
                "workspace `{}` does not declare a theme plugin",
                workspace.manifest().name()
            )
        })?;

        Ok(Self {
            engine,
            theme,
            hooks,
        })
    }

    /// Render an article using the plugins
    /// Returns the rendered HTML.
    pub fn render_article(&self, article: Article) -> eyre::Result<String> {
        let mut wit_article: WITArticle = article.into();

        for hook in &self.hooks {
            let (mut store, instance) = self.instantiate_hook(hook)?;
            wit_article = instance
                .thought_plugin_hook()
                .call_on_pre_render(&mut store, &wit_article)
                .map_err(|err| eyre!(err))?;
        }

        let html = {
            let (mut store, instance) = self.instantiate_theme()?;
            instance
                .thought_plugin_theme()
                .call_generate_page(&mut store, &wit_article)
                .map_err(|err| eyre!(err))?
        };

        let mut processed_html = html;
        for hook in &self.hooks {
            let (mut store, instance) = self.instantiate_hook(hook)?;
            processed_html = instance
                .thought_plugin_hook()
                .call_on_post_render(&mut store, &wit_article, &processed_html)
                .map_err(|err| eyre!(err))?;
        }

        Ok(processed_html)
    }

    /// Render the index using the theme plugin
    /// Returns the rendered HTML.
    pub fn render_index(&self, previews: Vec<ArticlePreview>) -> eyre::Result<String> {
        let (mut store, instance) = self.instantiate_theme()?;
        let wit_previews: Vec<WITArticlePreview> =
            previews.into_iter().map(|preview| preview.into()).collect();
        let rendered = instance
            .thought_plugin_theme()
            .call_generate_index(&mut store, &wit_previews)
            .map_err(|err| eyre!(err))?;
        Ok(rendered)
    }

    fn instantiate_theme(&self) -> eyre::Result<(Store<PluginInstanceState>, theme::ThemeRuntime)> {
        let mut store = self.new_store()?;
        let instance = self
            .theme
            .pre
            .instantiate(&mut store)
            .map_err(|err| eyre!(err))?;
        Ok((store, instance))
    }

    fn instantiate_hook(
        &self,
        handle: &HookHandle,
    ) -> eyre::Result<(Store<PluginInstanceState>, hook::HookRuntime)> {
        let mut store = self.new_store()?;
        let instance = handle
            .pre
            .instantiate(&mut store)
            .map_err(|err| eyre!(err))?;
        Ok((store, instance))
    }

    fn new_store(&self) -> eyre::Result<Store<PluginInstanceState>> {
        let ctx = WasiCtxBuilder::new().build();
        Ok(Store::new(&self.engine, PluginInstanceState::new(ctx)))
    }
}

fn build_engine() -> eyre::Result<WasmEngine> {
    let mut config = Config::new();
    config.wasm_component_model(true);
    config.wasm_reference_types(true);
    config.async_support(false);
    WasmEngine::new(&config).map_err(|err| eyre!(err))
}

fn instantiate_pre(
    engine: &WasmEngine,
    component: &Component,
) -> eyre::Result<wasmtime::component::InstancePre<PluginInstanceState>> {
    let mut linker = Linker::new(engine);
    wasmtime_wasi::p2::add_to_linker_sync(&mut linker).map_err(|err| eyre!(err))?;
    linker.instantiate_pre(component).map_err(|err| eyre!(err))
}

struct PluginInstanceState {
    wasi: WasiCtx,
    table: ResourceTable,
}

impl PluginInstanceState {
    fn new(wasi: WasiCtx) -> Self {
        Self {
            wasi,
            table: ResourceTable::new(),
        }
    }
}

impl WasiView for PluginInstanceState {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi,
            table: &mut self.table,
        }
    }
}
