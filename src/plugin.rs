use std::path::{Path, PathBuf};

use color_eyre::eyre::{self, eyre};
use sha2::{Digest, Sha256};
use tokio::fs;
use wasmtime::{
    Config, Engine as WasmEngine, InstanceAllocationStrategy, Store,
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
    theme_root: PathBuf,
    theme_fingerprint: String,
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
        let mut theme_root = None;

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
                    theme_root = Some(resolved.dir().to_path_buf());
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

        let theme_root_path = theme_root
            .as_ref()
            .expect("theme root missing after resolution")
            .to_path_buf();
        let theme_fingerprint = hash_theme_dir(&theme_root_path)?;

        Ok(Self {
            engine,
            theme,
            theme_root: theme_root_path,
            theme_fingerprint,
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

    /// Copy theme assets (if any) into the output directory.
    pub async fn copy_theme_assets(&self, output_root: impl AsRef<Path>) -> eyre::Result<()> {
        let source_assets = self.theme_root.join("assets");
        if !source_assets.exists() {
            return Ok(());
        }
        let target_assets = output_root.as_ref().join("assets");
        copy_dir_recursive(&source_assets, &target_assets)
            .await
            .map_err(|err| eyre!(err))
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

    pub fn theme_fingerprint(&self) -> &str {
        &self.theme_fingerprint
    }
}

fn build_engine() -> eyre::Result<WasmEngine> {
    let mut config = Config::new();
    config.wasm_component_model(true);
    config.wasm_reference_types(true);
    config.async_support(false);

    // Enable pooling allocator for faster instantiation
    config.allocation_strategy(InstanceAllocationStrategy::Pooling(Default::default()));

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

fn hash_theme_dir(root: &Path) -> eyre::Result<String> {
    let mut hasher = Sha256::new();
    hash_dir_recursive(root, root, &mut hasher)?;
    Ok(format!("{:x}", hasher.finalize()))
}

fn hash_dir_recursive(root: &Path, dir: &Path, hasher: &mut Sha256) -> eyre::Result<()> {
    let mut entries = std::fs::read_dir(dir)?
        .filter_map(Result::ok)
        .collect::<Vec<_>>();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let path = entry.path();
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str == "target" || name_str == ".git" {
            continue;
        }
        let rel = path.strip_prefix(root).unwrap_or(&path);
        let meta = entry.metadata()?;
        if meta.is_dir() {
            hash_dir_recursive(root, &path, hasher)?;
        } else if meta.is_file() {
            hasher.update(rel.to_string_lossy().as_bytes());
            let bytes = std::fs::read(&path)?;
            hasher.update(&bytes);
        }
    }
    Ok(())
}

async fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    let mut stack = vec![(src.to_path_buf(), dst.to_path_buf())];
    while let Some((current_src, current_dst)) = stack.pop() {
        let metadata = fs::metadata(&current_src).await?;
        if metadata.is_file() {
            if let Some(parent) = current_dst.parent() {
                fs::create_dir_all(parent).await?;
            }
            fs::copy(&current_src, &current_dst).await?;
            continue;
        }

        fs::create_dir_all(&current_dst).await?;
        let mut entries = fs::read_dir(&current_src).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            let target = current_dst.join(entry.file_name());
            let file_type = entry.file_type().await?;
            if file_type.is_dir() {
                stack.push((path, target));
            } else {
                if let Some(parent) = target.parent() {
                    fs::create_dir_all(parent).await?;
                }
                fs::copy(&path, &target).await?;
            }
        }
    }
    Ok(())
}

impl WasiView for PluginInstanceState {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi,
            table: &mut self.table,
        }
    }
}
