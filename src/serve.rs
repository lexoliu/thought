use std::{
    collections::HashMap,
    io,
    net::TcpListener,
    path::{Component, Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use color_eyre::eyre::{self, Report, eyre};
use futures::TryStreamExt;
use sha2::{Digest, Sha256};
use skyzen::{
    Body, Error as SkyError, Response, Result as SkyResult, StatusCode,
    header::{self, HeaderValue},
    routing::{CreateRouteNode, Params, Route, Router},
    runtime::native,
    utils::State,
};
use tokio::{fs as async_fs, sync::Mutex, task::spawn_blocking};
use tracing::info;

use crate::{
    article::{Article, ArticlePreview, FailToOpenArticle},
    cache::RenderCache,
    plugin::PluginManager,
    search,
    utils::write,
    workspace::Workspace,
};
use thought_plugin::helpers::{search_asset_dir, search_script_path, search_wasm_path};

type AsyncMutex<T> = Mutex<T>;

pub async fn serve(
    workspace: Workspace,
    host: String,
    port: u16,
    allow_fallback: bool,
) -> eyre::Result<()> {
    let port = select_port(&host, port, allow_fallback)?;
    let state = Arc::new(ServeState::new(workspace).await?);
    let address = format!("{host}:{port}");
    unsafe {
        // Safe because the server holds the only mutable reference to this env var.
        std::env::set_var("SKYZEN_ADDRESS", &address);
    }

    native::init_logging();

    spawn_blocking(move || {
        let state = state.clone();
        native::launch(move || {
            let router = build_router(state.clone());
            async move { router }
        });
    })
    .await
    .map_err(|err| eyre!(err))?;
    Ok(())
}

fn build_router(state: Arc<ServeState>) -> Router {
    Route::new((
        "/".at(index_handler),
        "/index.html".at(index_handler),
        "/{*path}".at(any_handler),
    ))
    .middleware(State(state))
    .build()
}

async fn index_handler(State(state): State<Arc<ServeState>>) -> SkyResult<Response> {
    state.serve_index().await.map_err(|err| map_error(err))
}

async fn any_handler(params: Params, State(state): State<Arc<ServeState>>) -> SkyResult<Response> {
    let path = params.get("path").unwrap_or("");
    state.serve_path(path).await.map_err(|err| map_error(err))
}

fn map_error(err: ServeError) -> SkyError {
    match err {
        ServeError::NotFound => SkyError::msg("Route not found").set_status(StatusCode::NOT_FOUND),
        ServeError::Internal(report) => {
            SkyError::msg(format!("{report:?}")).set_status(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

struct ServeState {
    workspace: Workspace,
    plugins: Arc<PluginManager>,
    cache: AsyncMutex<RenderCache>,
    article_guards: AsyncMutex<HashMap<String, Arc<AsyncMutex<()>>>>,
    index_lock: AsyncMutex<()>,
    index_dirty: AtomicBool,
    search_lock: AsyncMutex<()>,
    search_ready: AtomicBool,
    index_fingerprint: AsyncMutex<Option<String>>,
    theme_fingerprint: String,
}

impl ServeState {
    async fn new(workspace: Workspace) -> eyre::Result<Self> {
        async_fs::create_dir_all(workspace.build_dir()).await?;
        async_fs::create_dir_all(workspace.cache_dir()).await?;

        let plugins = PluginManager::resolve_workspace(&workspace).await?;
        let theme_fingerprint = plugins.theme_fingerprint().to_string();
        plugins
            .copy_theme_assets(workspace.build_dir())
            .await
            .map_err(|err| eyre!(err))?;
        let cache_path = workspace.cache_dir().join("cache.redb");
        let cache = RenderCache::load(cache_path).await?;
        let search_ready = async_fs::metadata(workspace.build_dir().join(search_script_path()))
            .await
            .is_ok()
            && async_fs::metadata(workspace.build_dir().join(search_wasm_path()))
                .await
                .is_ok();
        let index_exists = async_fs::metadata(workspace.build_dir().join("index.html"))
            .await
            .is_ok();

        let state = Self {
            workspace,
            plugins: Arc::new(plugins),
            cache: AsyncMutex::new(cache),
            article_guards: AsyncMutex::new(HashMap::new()),
            index_lock: AsyncMutex::new(()),
            index_dirty: AtomicBool::new(!index_exists),
            search_lock: AsyncMutex::new(()),
            search_ready: AtomicBool::new(search_ready),
            index_fingerprint: AsyncMutex::new(None),
            theme_fingerprint,
        };

        if !search_ready {
            state
                .ensure_search_assets()
                .await
                .map_err(|err| eyre!(format!("{err:?}")))?;
        }

        Ok(state)
    }

    async fn serve_index(&self) -> Result<Response, ServeError> {
        let path = self.ensure_index().await?;
        self.serve_file(&path).await
    }

    async fn serve_path(&self, raw_path: &str) -> Result<Response, ServeError> {
        if raw_path.is_empty() {
            return self.serve_index().await;
        }

        let sanitized = sanitize_relative_path(raw_path).ok_or(ServeError::NotFound)?;
        if sanitized.as_os_str().is_empty() {
            return self.serve_index().await;
        }

        if is_search_asset(&sanitized) {
            self.ensure_search_assets().await?;
        }

        if let Some(path) = self.resolve_static(&sanitized).await? {
            return self.serve_file(&path).await;
        }

        if sanitized.extension().and_then(|ext| ext.to_str()) == Some("html") {
            return self.render_article_for(&sanitized).await;
        }

        if sanitized.extension().is_none() {
            let html_candidate = sanitized.with_extension("html");
            match self.render_article_for(&html_candidate).await {
                Ok(resp) => return Ok(resp),
                Err(ServeError::NotFound) => {}
                Err(err) => return Err(err),
            }
        }

        Err(ServeError::NotFound)
    }

    async fn resolve_static(&self, relative: &Path) -> Result<Option<PathBuf>, ServeError> {
        let build_path = self.workspace.build_dir().join(relative);
        match async_fs::metadata(&build_path).await {
            Ok(meta) => {
                if meta.is_file() {
                    return Ok(Some(build_path));
                }
                if meta.is_dir() {
                    let index = build_path.join("index.html");
                    match async_fs::metadata(&index).await {
                        Ok(index_meta) if index_meta.is_file() => return Ok(Some(index)),
                        Ok(_) => return Ok(None),
                        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
                        Err(err) => return Err(err.into()),
                    }
                }
                Ok(None)
            }
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(err.into()),
        }
    }

    async fn render_article_for(&self, html_path: &Path) -> Result<Response, ServeError> {
        let segments = path_segments(html_path).ok_or(ServeError::NotFound)?;
        if segments.is_empty() {
            return Err(ServeError::NotFound);
        }
        let (segments, locale) =
            split_locale_from_segments(segments).ok_or(ServeError::NotFound)?;
        let guard = self.article_guard(&segments).await;
        let _lock = guard.lock().await;

        let article =
            Article::open_with_locale(self.workspace.clone(), segments.clone(), locale).await?;
        let html = self.render_article(article.clone()).await?;

        let output_path = self.workspace.build_dir().join(html_path);
        write(&output_path, html.as_bytes())
            .await
            .map_err(ServeError::from)?;
        self.index_dirty.store(true, Ordering::SeqCst);
        {
            let mut guard = self.index_fingerprint.lock().await;
            *guard = None;
        }
        self.search_ready.store(false, Ordering::SeqCst);

        Ok(html_response(html))
    }

    async fn fetch_cache_html(&self, article: &Article) -> Option<String> {
        let cache = self.cache.lock().await;
        cache.hit(article, &self.theme_fingerprint)
    }

    async fn store_cache_html(&self, article: &Article, html: &str) -> Result<(), ServeError> {
        let mut cache = self.cache.lock().await;
        cache.store(article, html, &self.theme_fingerprint);
        cache
            .persist()
            .await
            .map_err(|err| ServeError::Internal(err))?;
        Ok(())
    }

    async fn render_article(&self, article: Article) -> Result<String, ServeError> {
        if let Some(html) = self.fetch_cache_html(&article).await {
            return Ok(html);
        }

        let rendered = self
            .plugins
            .render_article(article.clone())
            .map_err(ServeError::internal)?;

        self.store_cache_html(&article, &rendered).await?;
        Ok(rendered)
    }

    async fn serve_file(&self, path: &Path) -> Result<Response, ServeError> {
        let data = async_fs::read(path).await.map_err(ServeError::from)?;
        let mut response = Response::new(Body::from(data));
        if let Some(value) = guess_content_type(path) {
            response.headers_mut().insert(header::CONTENT_TYPE, value);
        }
        Ok(response)
    }

    async fn ensure_index(&self) -> Result<PathBuf, ServeError> {
        let index_path = self.workspace.build_dir().join("index.html");
        if file_exists(&index_path).await? && !self.index_dirty.load(Ordering::SeqCst) {
            let current = self.compute_index_fingerprint().await?;
            let guard = self.index_fingerprint.lock().await;
            if guard.as_ref() == Some(&current) {
                return Ok(index_path);
            }
        }

        let _guard = self.index_lock.lock().await;
        if file_exists(&index_path).await? && !self.index_dirty.load(Ordering::SeqCst) {
            let current = self.compute_index_fingerprint().await?;
            let guard = self.index_fingerprint.lock().await;
            if guard.as_ref() == Some(&current) {
                return Ok(index_path);
            }
        }

        let previews = self.collect_previews().await?;
        let rendered = self
            .plugins
            .render_index(previews)
            .map_err(ServeError::internal)?;
        write(&index_path, rendered.as_bytes())
            .await
            .map_err(ServeError::from)?;
        self.index_dirty.store(false, Ordering::SeqCst);
        let fingerprint = self.compute_index_fingerprint().await?;
        {
            let mut guard = self.index_fingerprint.lock().await;
            *guard = Some(fingerprint);
        }
        Ok(index_path)
    }

    async fn collect_previews(&self) -> Result<Vec<ArticlePreview>, ServeError> {
        let mut previews = Vec::new();
        let mut stream = self.workspace.articles();
        while let Some(article) = stream.try_next().await.map_err(ServeError::internal)? {
            if article.is_default_locale() {
                previews.push(article.preview().clone());
            }
        }
        Ok(previews)
    }

    async fn ensure_search_assets(&self) -> Result<(), ServeError> {
        if self.search_ready.load(Ordering::SeqCst) && self.search_files_exist().await? {
            return Ok(());
        }
        let _guard = self.search_lock.lock().await;
        if self.search_ready.load(Ordering::SeqCst) && self.search_files_exist().await? {
            return Ok(());
        }
        let output = self.workspace.build_dir();
        search::emit_search_bundle(&self.workspace, &output, None)
            .await
            .map_err(ServeError::internal)?;
        self.search_ready.store(true, Ordering::SeqCst);
        Ok(())
    }

    async fn article_guard(&self, segments: &[String]) -> Arc<AsyncMutex<()>> {
        let key = segments.join("/");
        let mut guards = self.article_guards.lock().await;
        guards
            .entry(key)
            .or_insert_with(|| Arc::new(AsyncMutex::new(())))
            .clone()
    }

    async fn search_files_exist(&self) -> Result<bool, ServeError> {
        let js = self.workspace.build_dir().join(search_script_path());
        let wasm = self.workspace.build_dir().join(search_wasm_path());
        Ok(file_exists(&js).await? && file_exists(&wasm).await?)
    }

    async fn compute_index_fingerprint(&self) -> Result<String, ServeError> {
        let mut hasher = Sha256::new();
        let mut stream = self.workspace.articles();
        while let Some(article) = stream.try_next().await.map_err(ServeError::internal)? {
            if !article.is_default_locale() {
                continue;
            }
            hasher.update(article.output_file().as_bytes());
            hasher.update(article.sha256().as_bytes());
        }
        Ok(format!("{:x}", hasher.finalize()))
    }
}

#[derive(Debug)]
enum ServeError {
    NotFound,
    Internal(Report),
}

impl ServeError {
    fn internal(err: impl Into<Report>) -> Self {
        Self::Internal(err.into())
    }
}

impl From<FailToOpenArticle> for ServeError {
    fn from(err: FailToOpenArticle) -> Self {
        match err {
            FailToOpenArticle::ArticleNotFound => ServeError::NotFound,
            FailToOpenArticle::WorkspaceNotFound => {
                ServeError::internal(eyre!("Workspace not found"))
            }
            FailToOpenArticle::FailToOpenMetadata(inner) => ServeError::internal(eyre!(inner)),
        }
    }
}

impl From<std::io::Error> for ServeError {
    fn from(err: std::io::Error) -> Self {
        if err.kind() == io::ErrorKind::NotFound {
            ServeError::NotFound
        } else {
            ServeError::internal(err)
        }
    }
}

fn sanitize_relative_path(path: &str) -> Option<PathBuf> {
    let mut buf = PathBuf::new();
    for component in Path::new(path).components() {
        match component {
            Component::Normal(segment) => buf.push(segment),
            Component::CurDir | Component::RootDir | Component::Prefix(_) => {}
            Component::ParentDir => return None,
        }
    }
    Some(buf)
}

fn guess_content_type(path: &Path) -> Option<HeaderValue> {
    mime_guess::from_path(path)
        .first_raw()
        .and_then(|mime| HeaderValue::from_str(mime).ok())
}

async fn file_exists(path: &Path) -> Result<bool, ServeError> {
    match async_fs::metadata(path).await {
        Ok(meta) => Ok(meta.is_file()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(err.into()),
    }
}

fn html_response(html: String) -> Response {
    let mut response = Response::new(Body::from(html));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    response
}

fn path_segments(relative: &Path) -> Option<Vec<String>> {
    let mut trimmed = relative.to_path_buf();
    trimmed.set_extension("");
    if trimmed.as_os_str().is_empty() {
        return None;
    }

    let mut segments = Vec::new();
    for component in trimmed.components() {
        if let Component::Normal(segment) = component {
            segments.push(segment.to_string_lossy().to_string());
        } else {
            return None;
        }
    }
    Some(segments)
}

fn split_locale_from_segments(mut segments: Vec<String>) -> Option<(Vec<String>, Option<String>)> {
    if segments.is_empty() {
        return None;
    }
    let mut locale = None;
    if let Some(last) = segments.pop() {
        if let Some((slug, loc)) = last.split_once('.') {
            if !slug.is_empty() && !loc.is_empty() {
                segments.push(slug.to_string());
                locale = Some(loc.to_string());
            } else {
                segments.push(last);
            }
        } else {
            segments.push(last);
        }
    }
    Some((segments, locale))
}

fn select_port(host: &str, start: u16, allow_fallback: bool) -> eyre::Result<u16> {
    if !allow_fallback {
        return Ok(start);
    }

    for port in start..(start + 50) {
        let addr = format!("{host}:{port}");
        if TcpListener::bind(&addr).is_ok() {
            info!("Selected available port {}", port);
            return Ok(port);
        }
    }

    Err(eyre!("No available port found starting at {}", start))
}

fn is_search_asset(path: &Path) -> bool {
    let asset_dir = Path::new(search_asset_dir());
    path.starts_with(asset_dir)
}
