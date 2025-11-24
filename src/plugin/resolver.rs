use std::{
    fs as std_fs, io,
    io::Cursor,
    path::{Path, PathBuf},
};

use color_eyre::eyre::{bail, eyre};
use flate2::read::GzDecoder;
use git2::Repository;
use serde_json::Value;
use skyzen::{BodyError, HttpError, header};
use tar::Archive;
use thiserror::Error;
use tokio::{fs, process::Command, task};
use tracing::warn;
use url::Url;
use zenwave::{Client, ResponseExt, StatusCode, error::BoxHttpError};
use zip::ZipArchive;

use crate::{
    metadata::{FailToOpenMetadata, MetadataExt, PluginLocator, PluginManifest},
    utils::write,
    workspace::Workspace,
};

/// A resolved plugin ready to be built and used
#[derive(Debug)]
pub struct ResolvedPlugin {
    built: bool,       // Whether the plugin has been built
    force_build: bool, // Always rebuild even if wasm exists (local path)
    manifest: PluginManifest,
    // here is a `main.wasm` file under the dir, which can be executed via WASI preview 2
    dir: PathBuf,
}

impl ResolvedPlugin {
    /// Get the manifest of the plugin
    #[must_use]
    pub const fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    #[must_use]
    pub fn wasm_path(&self) -> PathBuf {
        self.dir().join("main.wasm")
    }

    /// Whether the plugin has been built
    #[must_use]
    pub const fn is_built(&self) -> bool {
        self.built
    }

    /// Build the plugin if it is not built yet
    pub async fn build(&mut self) -> color_eyre::eyre::Result<()> {
        if self.is_built() && !self.force_build {
            return Ok(());
        }

        let wasm_binary = self.wasm_path();
        run_component_build(&self.dir).await?;
        let artifact = locate_component_artifact(&self.dir).await?;
        fs::copy(&artifact, &wasm_binary).await?;

        self.built = true;
        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum ResolvePluginError {
    #[error("Fail to open plugin manifest: {0}")]
    FailToOpenPluginManifest(#[from] FailToOpenMetadata),
    #[error("I/O error while preparing plugin: {0}")]
    Io(#[from] io::Error),
    #[error("Network error while downloading plugin: {0}")]
    Network(#[from] zenwave::error::BoxHttpError),
    #[error("Git error: {0}")]
    Git(#[from] git2::Error),

    #[error("Fail to fetch GitHub release: {0}")]
    FailToFetchGitHubRelease(BodyError),
    #[error("Invalid plugin locator: {0}")]
    InvalidLocator(String),
}

pub async fn resolve_plugin(
    workspace: &Workspace,
    name: &str,
    locator: &PluginLocator,
) -> Result<ResolvedPlugin, ResolvePluginError> {
    let plugin_root = workspace.cache_dir().join("plugins");
    fs::create_dir_all(&plugin_root).await?;
    let normalized_name = normalize_name(name);
    let plugin_dir = plugin_root.join(&normalized_name);
    let locator_stamp = serde_json::to_vec(locator).expect("locator serialization failed");
    let descriptor_path = plugin_dir.join(".locator.json");
    let allow_reuse = !matches!(locator, PluginLocator::Local { .. });
    let mut reuse_existing = false;
    if allow_reuse && fs::metadata(&plugin_dir).await.is_ok() {
        if let Ok(existing) = fs::read(&descriptor_path).await {
            reuse_existing = existing == locator_stamp;
        }
    }

    if !reuse_existing && fs::metadata(&plugin_dir).await.is_ok() {
        fs::remove_dir_all(&plugin_dir).await?;
    }

    // prepare plugin to be used within the workspace's cache directory
    if !reuse_existing {
        match locator {
            PluginLocator::CratesIo { version } => {
                download_crate(name, version, &plugin_dir).await?;
            }
            PluginLocator::Git { url, rev, branch } => {
                if rev.is_some() && branch.is_some() {
                    return Err(ResolvePluginError::InvalidLocator(
                        "rev and branch cannot be set simultaneously".to_string(),
                    ));
                }
                if let Some((author, repo)) = parse_github(url) {
                    let tag = rev
                        .as_deref()
                        .or_else(|| branch.as_deref())
                        .unwrap_or("latest");
                    if try_github_release(&author, &repo, tag, &plugin_dir)
                        .await?
                        .is_none()
                    {
                        clone_repo(
                            url,
                            rev.as_deref().or_else(|| branch.as_deref()),
                            &plugin_dir,
                        )
                        .await?;
                    }
                } else {
                    clone_repo(
                        url,
                        rev.as_deref().or_else(|| branch.as_deref()),
                        &plugin_dir,
                    )
                    .await?;
                }
            }
            PluginLocator::Local { path } => {
                let source = fs::canonicalize(path).await?;
                copy_dir_recursive(&source, &plugin_dir).await?;
            }
            PluginLocator::Url { url } => {
                fetch_artifact(url, &plugin_dir).await?;
            }
        };
        if allow_reuse {
            write(&descriptor_path, &locator_stamp).await?;
        }
    }

    let dir = plugin_dir.clone();

    let manifest = PluginManifest::open(dir.join("Plugin.toml")).await?;
    let wasm_ready = fs::try_exists(dir.join("main.wasm")).await.unwrap_or(false);
    let force_build = matches!(locator, PluginLocator::Local { .. });

    Ok(ResolvedPlugin {
        built: wasm_ready && !force_build,
        force_build,
        manifest,
        dir,
    })
}

fn as_client_error<T>(err: T) -> BoxHttpError
where
    T: HttpError + 'static,
{
    Box::new(err)
}

async fn try_github_release(
    author: &str,
    repo: &str,
    tag: &str,
    target: &Path,
) -> Result<Option<PathBuf>, ResolvePluginError> {
    let mut client = zenwave::client();
    let api_url = if tag == "latest" {
        format!("https://api.github.com/repos/{author}/{repo}/releases/latest")
    } else {
        format!("https://api.github.com/repos/{author}/{repo}/releases/tags/{tag}")
    };
    let response = client
        .get(api_url)
        .header("User-Agent", "thought")
        .await
        .map_err(as_client_error)?;
    if response.status() == StatusCode::NOT_FOUND || !response.status().is_success() {
        return Ok(None);
    }

    let payload: Value = response
        .into_json()
        .await
        .map_err(ResolvePluginError::FailToFetchGitHubRelease)?;
    let Some(assets) = payload["assets"].as_array() else {
        return Ok(None);
    };

    for asset in assets {
        let Some(name) = asset["name"].as_str() else {
            continue;
        };
        let Some(download_url) = asset["browser_download_url"].as_str() else {
            continue;
        };

        let bytes = client
            .get(download_url)
            .header(header::USER_AGENT, "thought")
            .bytes()
            .await
            .map_err(as_client_error)?;

        fs::create_dir_all(target).await?;
        if name.ends_with(".wasm") {
            let wasm_path = target.join("main.wasm");
            fs::write(&wasm_path, bytes.as_ref()).await?;
            return Ok(Some(target.to_path_buf()));
        }
        if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
            unpack_tarball(bytes.as_ref(), target).await?;
            flatten_directory(target).await?;
            return Ok(Some(target.to_path_buf()));
        }
        if name.ends_with(".zip") {
            unpack_zip(bytes.as_ref(), target).await?;
            flatten_directory(target).await?;
            return Ok(Some(target.to_path_buf()));
        }
    }

    Ok(None)
}

fn normalize_name(name: &str) -> String {
    name.chars()
        .map(|ch| match ch {
            '/' | '\\' | ':' => '-',
            _ => ch,
        })
        .collect()
}

async fn copy_dir_recursive(src: &Path, dst: &Path) -> io::Result<()> {
    let src = src.to_path_buf();
    let dst = dst.to_path_buf();
    task::spawn_blocking(move || copy_dir_recursive_sync(&src, &dst))
        .await
        .map_err(io::Error::other)?
}

fn copy_dir_recursive_sync(src: &Path, dst: &Path) -> io::Result<()> {
    std_fs::create_dir_all(dst)?;
    for entry in std_fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let target = dst.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir_recursive_sync(&entry.path(), &target)?;
        } else if file_type.is_file() {
            if let Some(parent) = target.parent() {
                std_fs::create_dir_all(parent)?;
            }
            std_fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

async fn download_crate(
    name: &str,
    version: &str,
    target: &Path,
) -> Result<(), ResolvePluginError> {
    let mut client = zenwave::client();
    let url = format!("https://crates.io/api/v1/crates/{name}/{version}/download");
    let bytes = client
        .get(url)
        .header("User-Agent", "thought")
        .bytes()
        .await
        .map_err(as_client_error)?;
    fs::create_dir_all(target).await?;
    unpack_tarball(bytes.as_ref(), target).await?;
    flatten_directory(target).await?;
    Ok(())
}

async fn fetch_artifact(url: &str, target: &Path) -> Result<(), ResolvePluginError> {
    let parsed =
        Url::parse(url).map_err(|err| ResolvePluginError::InvalidLocator(err.to_string()))?;
    let bytes = match parsed.scheme() {
        "file" => {
            let path = parsed.to_file_path().map_err(|_| {
                ResolvePluginError::InvalidLocator(format!("Invalid file:// url: {url}"))
            })?;
            fs::read(path).await?
        }
        "http" | "https" => {
            if parsed.scheme() == "http" {
                warn!("Using insecure HTTP to download plugin artifact: {}", url);
            }
            let mut client = zenwave::client();
            client
                .get(url.to_string())
                .header("User-Agent", "thought")
                .bytes()
                .await
                .map_err(as_client_error)?
                .to_vec()
        }
        other => {
            return Err(ResolvePluginError::InvalidLocator(format!(
                "Unsupported URL scheme `{other}`"
            )));
        }
    };
    fs::create_dir_all(target).await?;

    let lower = url.to_ascii_lowercase();
    if lower.ends_with(".wasm") {
        fs::write(target.join("main.wasm"), &bytes).await?;
        return Ok(());
    }
    if lower.ends_with(".tar.gz") || lower.ends_with(".tgz") {
        unpack_tarball(bytes.as_ref(), target).await?;
        flatten_directory(target).await?;
        return Ok(());
    }
    if lower.ends_with(".zip") {
        unpack_zip(bytes.as_ref(), target).await?;
        flatten_directory(target).await?;
        return Ok(());
    }

    Err(ResolvePluginError::InvalidLocator(format!(
        "Unsupported artifact url: {url}"
    )))
}

async fn unpack_tarball(bytes: &[u8], target: &Path) -> io::Result<()> {
    let data = bytes.to_vec();
    let target = target.to_path_buf();
    task::spawn_blocking(move || -> io::Result<()> {
        let decoder = GzDecoder::new(&data[..]);
        let mut archive = Archive::new(decoder);
        archive.unpack(&target)?;
        Ok(())
    })
    .await
    .map_err(io::Error::other)??;
    Ok(())
}

async fn unpack_zip(bytes: &[u8], target: &Path) -> io::Result<()> {
    let data = bytes.to_vec();
    let target = target.to_path_buf();
    task::spawn_blocking(move || -> io::Result<()> {
        let reader = Cursor::new(data);
        let mut archive =
            ZipArchive::new(reader).map_err(|err| io::Error::other(format!("{err:?}")))?;
        archive
            .extract(&target)
            .map_err(|err| io::Error::other(format!("{err:?}")))?;
        Ok(())
    })
    .await
    .map_err(io::Error::other)??;
    Ok(())
}

async fn flatten_directory(dir: &Path) -> io::Result<()> {
    let dir = dir.to_path_buf();
    task::spawn_blocking(move || flatten_directory_sync(&dir))
        .await
        .map_err(io::Error::other)?
}

fn flatten_directory_sync(dir: &Path) -> io::Result<()> {
    let mut entries = std_fs::read_dir(dir)?
        .filter_map(Result::ok)
        .collect::<Vec<_>>();
    if entries.len() != 1 {
        return Ok(());
    }
    let first = entries.pop().unwrap();
    if first.file_type()?.is_dir() {
        let inner = first.path();
        for entry in std_fs::read_dir(&inner)? {
            let entry = entry?;
            std_fs::rename(entry.path(), dir.join(entry.file_name()))?;
        }
        std_fs::remove_dir_all(inner)?;
    }
    Ok(())
}

async fn clone_repo(url: &str, rev: Option<&str>, target: &Path) -> Result<(), ResolvePluginError> {
    let repo_url = url.to_string();
    let rev = rev.map(str::to_owned);
    let target = target.to_path_buf();
    task::spawn_blocking(move || {
        let repo = Repository::clone(&repo_url, &target)?;
        if let Some(revision) = rev {
            checkout_revision(&repo, &revision)?;
        }
        Ok::<(), ResolvePluginError>(())
    })
    .await
    .map_err(io::Error::other)??;
    Ok(())
}

fn checkout_revision(repo: &Repository, rev: &str) -> Result<(), ResolvePluginError> {
    let (object, reference) = repo.revparse_ext(rev)?;
    repo.checkout_tree(&object, None)?;
    if let Some(reference) = reference {
        repo.set_head(reference.name().unwrap_or("HEAD"))?;
    } else {
        repo.set_head_detached(object.id())?;
    }
    Ok(())
}

fn parse_github(url: &str) -> Option<(String, String)> {
    let parsed = Url::parse(url).ok()?;
    if parsed.host_str()? != "github.com" {
        return None;
    }
    let mut segments = parsed.path_segments()?;
    let author = segments.next()?.to_string();
    let mut repo = segments.next()?.to_string();
    if repo.ends_with(".git") {
        repo.truncate(repo.len() - 4);
    }
    Some((author, repo))
}

async fn run_component_build(dir: &Path) -> color_eyre::eyre::Result<()> {
    // Check if `wasm32-wasip2` target is installed
    let target_list_output = Command::new("rustup")
        .arg("target")
        .arg("list")
        .arg("--installed")
        .output()
        .await?;
    let installed_targets = String::from_utf8_lossy(&target_list_output.stdout);
    if !installed_targets
        .lines()
        .any(|line| line.trim() == "wasm32-wasip2")
    {
        bail!(
            "The target `wasm32-wasip2` is not installed. Please run `rustup target add wasm32-wasip2` to install it."
        );
    }

    let status = Command::new("cargo")
        .arg("build")
        .arg("--release")
        // DO NOT use `cargo component build`, use standard cargo build, it has already built-in support for wasm32-wasip2 target
        .arg("--target")
        .arg("wasm32-wasip2")
        // use Cargo.toml in the plugin directory
        .arg("--manifest-path")
        .arg(dir.join("Cargo.toml"))
        .current_dir(dir)
        .status()
        .await?;
    if !status.success() {
        return Err(eyre!(
            "Failed to build plugin in {} (exit code {status})",
            dir.display()
        ));
    }
    Ok(())
}

async fn locate_component_artifact(dir: &Path) -> io::Result<PathBuf> {
    let candidates = [
        dir.join("target/wasm32-wasip2/release"),
        dir.join("target/wasm32-wasip2/debug"),
    ];
    for candidate in candidates {
        if let Ok(mut entries) = fs::read_dir(&candidate).await {
            while let Some(entry) = entries.next_entry().await? {
                if entry.file_type().await?.is_file()
                    && entry.path().extension().and_then(|ext| ext.to_str()) == Some("wasm")
                {
                    return Ok(entry.path());
                }
            }
        }
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        "Unable to locate built wasm artifact",
    ))
}
