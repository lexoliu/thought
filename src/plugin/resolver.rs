use std::{
    fs as std_fs, io,
    path::{Path, PathBuf},
};

use color_eyre::eyre::eyre;
use flate2::read::GzDecoder;
use git2::Repository;
use reqwest::Client;
use serde_json::Value;
use tar::Archive;
use thiserror::Error;
use tokio::{fs, process::Command, task};
use url::Url;

use crate::{
    metadata::{FailToOpenMetadata, MetadataExt, PluginLocator, PluginManifest},
    workspace::Workspace,
};

/// A resolved plugin ready to be built and used
#[derive(Debug)]
pub struct ResolvedPlugin {
    built: bool, // Whether the plugin has been built
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
        if self.is_built() {
            return Ok(());
        }

        let wasm_binary = self.wasm_path();
        if fs::metadata(&wasm_binary).await.is_err() {
            run_component_build(&self.dir).await?;
            if fs::metadata(&wasm_binary).await.is_err() {
                let artifact = locate_component_artifact(&self.dir).await?;
                fs::copy(&artifact, &wasm_binary).await?;
            }
        }

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
    Network(#[from] reqwest::Error),
    #[error("Git error: {0}")]
    Git(#[from] git2::Error),
}

pub async fn resolve_plugin(
    workspace: &Workspace,
    name: &str,
    locator: &PluginLocator,
) -> Result<ResolvedPlugin, ResolvePluginError> {
    let built = false;
    let plugin_root = workspace.cache_dir().join("plugins");
    fs::create_dir_all(&plugin_root).await?;
    let normalized_name = normalize_name(name);
    let plugin_dir = plugin_root.join(&normalized_name);
    let client = Client::new();

    if fs::metadata(&plugin_dir).await.is_ok() {
        fs::remove_dir_all(&plugin_dir).await?;
    }

    // prepare plugin to be used within the workspace's cache directory
    let dir: PathBuf = match locator {
        PluginLocator::CratesIo { version } => {
            download_crate(&client, name, version, &plugin_dir).await?;
            plugin_dir.clone()
        }
        PluginLocator::Git { url, rev } => {
            if let Some((author, repo)) = parse_github(url) {
                let tag = rev.as_deref().unwrap_or("latest");
                if let Some(resolved) =
                    try_github_release(&client, &author, &repo, tag, &plugin_dir).await?
                {
                    resolved
                } else {
                    clone_repo(url, rev.as_deref(), &plugin_dir).await?;
                    plugin_dir.clone()
                }
            } else {
                clone_repo(url, rev.as_deref(), &plugin_dir).await?;
                plugin_dir.clone()
            }
        }
        PluginLocator::Local { path } => {
            let source = fs::canonicalize(path).await?;
            copy_dir_recursive(&source, &plugin_dir).await?;
            plugin_dir.clone()
        }
    };

    let manifest = PluginManifest::open(dir.join("Plugin.toml")).await?;

    Ok(ResolvedPlugin {
        built,
        manifest,
        dir,
    })
}

async fn try_github_release(
    client: &Client,
    author: &str,
    repo: &str,
    tag: &str,
    target: &Path,
) -> Result<Option<PathBuf>, ResolvePluginError> {
    let api_url = if tag == "latest" {
        format!("https://api.github.com/repos/{author}/{repo}/releases/latest")
    } else {
        format!("https://api.github.com/repos/{author}/{repo}/releases/tags/{tag}")
    };
    let response = client
        .get(api_url)
        .header("User-Agent", "thought")
        .send()
        .await?;
    if !response.status().is_success() {
        return Ok(None);
    }

    let payload: Value = response.json().await?;
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
            .header("User-Agent", "thought")
            .send()
            .await?
            .bytes()
            .await?;

        fs::create_dir_all(target).await?;
        if name.ends_with(".wasm") {
            let wasm_path = target.join("main.wasm");
            fs::write(&wasm_path, &bytes).await?;
            return Ok(Some(target.to_path_buf()));
        }
        if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
            unpack_tarball(&bytes, target).await?;
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
    client: &Client,
    name: &str,
    version: &str,
    target: &Path,
) -> Result<(), ResolvePluginError> {
    let url = format!("https://crates.io/api/v1/crates/{name}/{version}/download");
    let response = client
        .get(url)
        .header("User-Agent", "thought")
        .send()
        .await?;
    let bytes = response.bytes().await?;
    fs::create_dir_all(target).await?;
    unpack_tarball(&bytes, target).await?;
    flatten_directory(target).await?;
    Ok(())
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
    let status = Command::new("cargo")
        .arg("build")
        .arg("--release")
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
