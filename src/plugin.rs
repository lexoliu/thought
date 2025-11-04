use crate::types::metadata::{
    BuildMode, PluginKind, PluginLocator, PluginManifest, PluginSpec, PluginToml,
};
use anyhow::{Result, anyhow};
use git2::Repository;
use reqwest::Client;
use serde::Deserialize;
use std::{
    env::temp_dir,
    fs, io,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
};
use tokio::sync::Mutex;
use url::Url;
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
    theme_manifest: PluginManifest,
    theme_assets: Option<PathBuf>,
    hooks: Vec<HookRuntimeEntry>,
}

struct HookRuntimeEntry {
    manifest: PluginManifest,
    runtime: Arc<Mutex<PluginRuntime>>,
}

#[derive(Debug, thiserror::Error)]
pub enum PluginLoadError {
    #[error("Plugin.toml not found at {0}")]
    MissingPluginManifest(PathBuf),
    #[error("{0}")]
    Water(#[from] crate::types::metadata::WaterError),
    #[error("{0}")]
    Manifest(#[from] crate::types::metadata::ManifestError),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("network error: {0}")]
    Network(#[from] reqwest::Error),
    #[error("git error: {0}")]
    Git(#[from] git2::Error),
    #[error("missing main.wasm for plugin `{0}` at {1:?}")]
    MissingBinary(String, PathBuf),
    #[error("plugin `{name}` declared as {expected:?} but manifest reports {actual:?}")]
    IncompatibleKind {
        name: String,
        expected: PluginKind,
        actual: PluginKind,
    },
    #[error("multiple themes detected; already loaded `{0}`")]
    ThemeAlreadyLoaded(String),
    #[error("no theme plugin resolved from Plugin.toml")]
    ThemeMissing,
    #[error("failed to initialize plugin runtime: {0}")]
    Runtime(anyhow::Error),
    #[error("unsupported plugin specification: {0}")]
    UnsupportedSpec(String),
    #[error("failed to unpack archive: {0}")]
    Archive(String),
}

impl PluginManager {
    pub async fn from_workspace(workspace: Workspace) -> Result<Self, PluginLoadError> {
        PluginLoader::load(workspace).await
    }

    pub fn workspace(&self) -> &Workspace {
        &self.workspace
    }

    pub fn theme_runtime(&self) -> Arc<ThemeRuntime> {
        self.theme.clone()
    }

    pub fn theme_manifest(&self) -> &PluginManifest {
        &self.theme_manifest
    }

    pub fn hook_manifests(&self) -> impl Iterator<Item = &PluginManifest> {
        self.hooks.iter().map(|hook| &hook.manifest)
    }

    pub fn copy_theme_assets(&self, destination: impl AsRef<Path>) -> io::Result<()> {
        if let Some(assets) = &self.theme_assets {
            let destination = destination.as_ref();
            if destination.exists() {
                fs::remove_dir_all(destination)?;
            }
            copy_directory_recursively(assets, destination)?;
        }
        Ok(())
    }

    pub fn plugins(&self) -> impl Iterator<Item = Arc<Mutex<PluginRuntime>>> + '_ {
        self.hooks.iter().map(|hook| hook.runtime.clone())
    }

    /// Apply the `on_pre_render` lifecycle hook of each plugin in declaration order.
    pub async fn apply_pre_render(&self, article: Article) -> Result<Article> {
        let mut current = article;
        for hook in &self.hooks {
            let mut plugin = hook.runtime.lock().await;
            current = plugin.on_pre_render(&current)?;
        }
        Ok(current)
    }

    /// Apply the `on_post_render` hook sequentially, letting each plugin transform the HTML.
    pub async fn apply_post_render(&self, article: &Article, html: String) -> Result<String> {
        let mut current = html;
        for hook in &self.hooks {
            let mut plugin = hook.runtime.lock().await;
            current = plugin.on_post_render(article, &current)?;
        }
        Ok(current)
    }
}

struct PluginLoader {
    workspace: Workspace,
    workspace_root: PathBuf,
    cache_root: PathBuf,
    sources_root: PathBuf,
    build_root: PathBuf,
    http: Client,
}

struct ResolvedPlugin {
    manifest: PluginManifest,
    binary: Vec<u8>,
    assets_path: PathBuf,
    spec: PluginSpec,
}

impl PluginLoader {
    async fn load(workspace: Workspace) -> Result<PluginManager, PluginLoadError> {
        let workspace_root = workspace.path().to_path_buf();
        let plugin_path = workspace_root.join("Plugin.toml");
        if !plugin_path.exists() {
            return Err(PluginLoadError::MissingPluginManifest(plugin_path));
        }
        let plugin = PluginToml::load(&plugin_path)?;
        let specs = plugin.plugins()?;

        let cache_root = workspace_root.join(".cache");
        fs::create_dir_all(&cache_root)?;
        let sources_root = cache_root.join("plugins");
        fs::create_dir_all(&sources_root)?;
        let build_root = workspace_root.join("build");
        fs::create_dir_all(&build_root)?;

        let http = Client::builder()
            .user_agent("thought-plugin-manager/0.1")
            .build()
            .map_err(PluginLoadError::Network)?;

        let mut loader = Self {
            workspace,
            workspace_root,
            cache_root,
            sources_root,
            build_root,
            http,
        };
        loader.resolve(specs).await
    }

    async fn resolve(&mut self, specs: Vec<PluginSpec>) -> Result<PluginManager, PluginLoadError> {
        let mut theme_slot: Option<(ThemeRuntime, PluginManifest, Option<PathBuf>)> = None;
        let mut hooks = Vec::new();

        for spec in specs {
            let resolved = self.resolve_single(spec).await?;
            if let Some(expected) = resolved.spec.declared_kind
                && expected != resolved.manifest.kind
            {
                return Err(PluginLoadError::IncompatibleKind {
                    name: resolved.manifest.name.clone(),
                    expected,
                    actual: resolved.manifest.kind.clone(),
                });
            }

            match resolved.manifest.kind {
                PluginKind::Theme => {
                    if let Some((_, manifest, _)) = &theme_slot {
                        return Err(PluginLoadError::ThemeAlreadyLoaded(manifest.name.clone()));
                    }
                    let runtime =
                        ThemeRuntime::new(resolved.manifest.name.clone(), &resolved.binary)
                            .map_err(PluginLoadError::Runtime)?;
                    let assets = if resolved.assets_path.exists() {
                        Some(resolved.assets_path.clone())
                    } else {
                        None
                    };
                    theme_slot = Some((runtime, resolved.manifest, assets));
                }
                PluginKind::Hook => {
                    let runtime = PluginRuntime::new(
                        resolved.manifest.name.clone(),
                        &resolved.binary,
                        &self.cache_root,
                        &resolved.assets_path,
                        Some(&self.build_root),
                    )
                    .await
                    .map_err(PluginLoadError::Runtime)?;
                    hooks.push(HookRuntimeEntry {
                        manifest: resolved.manifest,
                        runtime: Arc::new(Mutex::new(runtime)),
                    });
                }
            }
        }

        let (theme_runtime, theme_manifest, theme_assets) =
            theme_slot.ok_or(PluginLoadError::ThemeMissing)?;

        Ok(PluginManager {
            workspace: self.workspace.clone(),
            theme: Arc::new(theme_runtime),
            theme_manifest,
            theme_assets,
            hooks,
        })
    }

    async fn resolve_single(
        &mut self,
        spec: PluginSpec,
    ) -> Result<ResolvedPlugin, PluginLoadError> {
        let root = self.prepare_source_dir(&spec).await?;
        let manifest = PluginManifest::load(root.join("Plugin.toml"))?;
        let assets_path = self.ensure_assets_dir(&root)?;
        let wasm_path = root.join("main.wasm");

        let needs_compilation = match spec.mode {
            BuildMode::Cargo => true,
            BuildMode::Precompiled => !wasm_path.exists(),
        };

        if needs_compilation {
            self.compile_plugin(&manifest, &root)?;
        }

        if !wasm_path.exists() {
            return Err(PluginLoadError::MissingBinary(
                manifest.name.clone(),
                wasm_path,
            ));
        }

        let binary = fs::read(&wasm_path)?;

        Ok(ResolvedPlugin {
            manifest,
            binary,
            assets_path,
            spec,
        })
    }

    async fn prepare_source_dir(&self, spec: &PluginSpec) -> Result<PathBuf, PluginLoadError> {
        match &spec.locator {
            PluginLocator::Local(path) => {
                let path = if path.is_relative() {
                    self.workspace_root.join(path)
                } else {
                    path.clone()
                };
                if !path.exists() {
                    return Err(PluginLoadError::UnsupportedSpec(format!(
                        "local plugin directory `{}` does not exist",
                        path.display()
                    )));
                }
                Ok(path)
            }
            PluginLocator::CratesIo { version } => {
                let dest = self.sources_root.join(format!("{}-{}", spec.name, version));
                self.fetch_from_crates_io(&spec.name, version, &dest)
                    .await?;
                Ok(dest)
            }
            PluginLocator::Git {
                repo,
                rev,
                is_github,
            } => {
                let dest = self.sources_root.join(&spec.name);
                self.fetch_from_git(repo, rev.as_deref(), *is_github, &dest, spec.mode)
                    .await?;
                Ok(dest)
            }
        }
    }

    fn ensure_assets_dir(&self, root: &Path) -> Result<PathBuf, PluginLoadError> {
        let assets_path = root.join("assets");
        if !assets_path.exists() {
            fs::create_dir_all(&assets_path)?;
        }
        Ok(assets_path)
    }

    fn compile_plugin(
        &self,
        manifest: &PluginManifest,
        root: &Path,
    ) -> Result<(), PluginLoadError> {
        let cargo_path = root.join("Cargo.toml");
        if !cargo_path.exists() {
            return Err(PluginLoadError::MissingBinary(
                manifest.name.clone(),
                root.join("main.wasm"),
            ));
        }

        let target = match manifest.kind {
            PluginKind::Theme => "wasm32-unknown-unknown",
            PluginKind::Hook => "wasm32-wasip2",
        };

        let target_dir = self
            .cache_root
            .join("target")
            .join(&manifest.name)
            .join(target);
        fs::create_dir_all(&target_dir)?;

        let status = Command::new("cargo")
            .arg("build")
            .arg("--release")
            .arg("--target")
            .arg(target)
            .current_dir(root)
            .env(
                "CARGO_TARGET_DIR",
                target_dir.parent().unwrap_or(&target_dir),
            )
            .status()
            .map_err(|err| PluginLoadError::Runtime(err.into()))?;

        if !status.success() {
            return Err(PluginLoadError::Runtime(anyhow!(
                "cargo build failed for plugin `{}`",
                manifest.name
            )));
        }

        let artifact_dir = target_dir.join("release");
        let wasm_file = find_single_wasm(&artifact_dir).ok_or_else(|| {
            PluginLoadError::MissingBinary(manifest.name.clone(), artifact_dir.join("*.wasm"))
        })?;
        let main_wasm = root.join("main.wasm");
        fs::copy(wasm_file, main_wasm)?;
        Ok(())
    }

    async fn fetch_from_crates_io(
        &self,
        name: &str,
        version: &str,
        dest: &Path,
    ) -> Result<(), PluginLoadError> {
        if dest.exists() {
            fs::remove_dir_all(dest)?;
        }
        let repo = self.repo_from_crates(name, version).await?;
        self.fetch_from_git(
            &repo,
            Some(version),
            repo.contains("github.com"),
            dest,
            BuildMode::Precompiled,
        )
        .await?;
        Ok(())
    }

    async fn repo_from_crates(&self, name: &str, version: &str) -> Result<String, PluginLoadError> {
        #[derive(Deserialize)]
        struct CrateResponse {
            #[serde(rename = "crate")]
            krate: CrateMetadata,
        }

        #[derive(Deserialize)]
        struct CrateMetadata {
            repository: Option<String>,
        }

        let url = format!("https://crates.io/api/v1/crates/{name}/{version}");
        let response = self.http.get(url).send().await?;
        if !response.status().is_success() {
            return Err(PluginLoadError::UnsupportedSpec(format!(
                "failed to fetch crate `{name}` metadata (status {})",
                response.status(),
            )));
        }
        let metadata: CrateResponse = response.json().await?;
        let repo = metadata.krate.repository.ok_or_else(|| {
            PluginLoadError::UnsupportedSpec(format!(
                "crate `{name}` does not declare a repository"
            ))
        })?;
        Ok(repo)
    }

    async fn fetch_from_git(
        &self,
        repo: &str,
        rev: Option<&str>,
        is_github: bool,
        dest: &Path,
        mode: BuildMode,
    ) -> Result<(), PluginLoadError> {
        if dest.exists() {
            fs::remove_dir_all(dest)?;
        }
        self.clone_repo(repo, rev, dest)?;

        if is_github
            && mode == BuildMode::Precompiled
            && let Some(result) = self.try_download_github_release(repo, rev, dest).await
        {
            result?;
        }

        Ok(())
    }

    fn clone_repo(
        &self,
        repo: &str,
        rev: Option<&str>,
        dest: &Path,
    ) -> Result<(), PluginLoadError> {
        let repo = Repository::clone(repo, dest)?;
        if let Some(rev) = rev {
            let obj = repo.revparse_single(rev)?;
            repo.checkout_tree(&obj, None)?;
            if let Ok(commit) = obj.peel_to_commit() {
                repo.set_head_detached(commit.id())?;
            } else {
                repo.set_head_detached(obj.id())?;
            }
        }
        Ok(())
    }

    async fn try_download_github_release(
        &self,
        repo_url: &str,
        rev: Option<&str>,
        dest: &Path,
    ) -> Option<Result<PathBuf, PluginLoadError>> {
        let url = Url::parse(repo_url).ok()?;
        if url.host_str() != Some("github.com") {
            return None;
        }
        let mut segments = url.path_segments()?;
        let owner = segments.next()?.to_string();
        let repo = segments.next()?.trim_end_matches(".git").to_string();
        let release_api = if let Some(tag) = rev {
            format!("https://api.github.com/repos/{owner}/{repo}/releases/tags/{tag}")
        } else {
            format!("https://api.github.com/repos/{owner}/{repo}/releases/latest")
        };

        match self.download_release_asset(&release_api, dest).await {
            Ok(path) => Some(Ok(path)),
            Err(PluginLoadError::UnsupportedSpec(_)) => None,
            Err(PluginLoadError::Network(_)) => None,
            Err(err) => Some(Err(err)),
        }
    }

    async fn download_release_asset(
        &self,
        api_url: &str,
        dest: &Path,
    ) -> Result<PathBuf, PluginLoadError> {
        #[derive(Deserialize)]
        struct ReleaseAsset {
            name: String,
            browser_download_url: String,
        }

        #[derive(Deserialize)]
        struct ReleaseResponse {
            assets: Vec<ReleaseAsset>,
        }

        let response = self.http.get(api_url).send().await?;
        if !response.status().is_success() {
            return Err(PluginLoadError::UnsupportedSpec(format!(
                "no GitHub release available at {api_url} (status {})",
                response.status()
            )));
        }
        let release: ReleaseResponse = response.json().await?;
        let asset = release
            .assets
            .into_iter()
            .find(|asset| asset.name.ends_with(".wasm"))
            .ok_or_else(|| {
                PluginLoadError::UnsupportedSpec(format!(
                    "release response at {api_url} does not contain a `.wasm` asset"
                ))
            })?;
        let bytes = self
            .http
            .get(asset.browser_download_url)
            .send()
            .await?
            .bytes()
            .await?;
        let wasm_path = dest.join("main.wasm");
        if let Some(parent) = wasm_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&wasm_path, &bytes)?;
        Ok(wasm_path)
    }
}

fn find_single_wasm(dir: &Path) -> Option<PathBuf> {
    let entries = fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().map(|ext| ext == "wasm").unwrap_or(false) {
            return Some(path);
        }
    }
    None
}

fn copy_directory_recursively(src: &Path, dest: &Path) -> io::Result<()> {
    if !src.exists() {
        return Ok(());
    }
    fs::create_dir_all(dest)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let src_path = entry.path();
        let dest_path = dest.join(entry.file_name());
        if file_type.is_dir() {
            copy_directory_recursively(&src_path, &dest_path)?;
        } else if file_type.is_file() {
            fs::copy(&src_path, &dest_path)?;
        }
    }
    Ok(())
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
        let result = self.bindings.thought_plugin_hook().call_on_post_render(
            &mut self.store,
            &input,
            html,
        )?;
        Ok(result)
    }
}
