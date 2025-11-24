use std::{collections::BTreeMap, path::Path, path::PathBuf};

use clap::Subcommand;
use color_eyre::eyre;
use flate2::{Compression, write::GzEncoder};
use serde::Serialize;
use sha2::{Digest, Sha256};
use tar::Builder;
use thought::{plugin::PluginManager, workspace::Workspace};
use tokio::{fs, process::Command};
use toml::Value;
use whoami;

#[derive(Subcommand)]
pub enum PluginCommands {
    /// Create a new plugin scaffold (theme or hook)
    Create {
        name: String,
        #[arg(long, default_value = "theme")]
        kind: String,
        #[arg(long)]
        path: Option<PathBuf>,
    },
    /// Build and package a plugin into a tar.gz artifact
    Package {
        #[arg(long, default_value = ".")]
        path: PathBuf,
        #[arg(long)]
        out: Option<PathBuf>,
    },

    /// Re-fetch and rebuild a plugin declared in Thought.toml
    Update {
        /// Plugin name as declared in Thought.toml
        name: String,
    },
}

const TEMPLATE_THEME_LIB: &str = include_str!("templates/theme_lib.rs");
const TEMPLATE_HOOK_LIB: &str = include_str!("templates/hook_lib.rs");
const TEMPLATE_THEME_INDEX: &str = include_str!("templates/theme_index.html");
const TEMPLATE_THEME_ARTICLE: &str = include_str!("templates/theme_article.html");
const TEMPLATE_STYLE: &str = include_str!("templates/style.css");
const TEMPLATE_GITIGNORE: &str = include_str!("templates/gitignore");

#[derive(Serialize)]
struct CargoToml {
    package: PackageSection,
    lib: LibSection,
    dependencies: BTreeMap<String, toml::Value>,
}

#[derive(Serialize)]
struct PackageSection {
    name: String,
    version: String,
    edition: String,
}

#[derive(Serialize)]
struct LibSection {
    #[serde(rename = "crate-type")]
    crate_type: Vec<String>,
}

#[derive(Serialize)]
struct PluginToml {
    name: String,
    author: String,
    version: String,
    #[serde(rename = "type")]
    kind: String,
}

pub async fn handle_plugin_command(cmd: PluginCommands) -> eyre::Result<()> {
    match cmd {
        PluginCommands::Create { name, kind, path } => {
            plugin_create(&name, &kind, path.as_deref()).await?;
            Ok(())
        }
        PluginCommands::Package { path, out } => {
            plugin_package(&path, out.as_deref()).await?;
            Ok(())
        }
        PluginCommands::Update { name } => {
            plugin_update(&name).await?;
            Ok(())
        }
    }
}

async fn plugin_create(name: &str, kind: &str, path: Option<&Path>) -> eyre::Result<()> {
    let kind = kind.to_lowercase();
    if kind != "theme" && kind != "hook" {
        return Err(eyre::eyre!("Unsupported plugin kind: {kind}"));
    }

    let root = match path {
        Some(p) => p.to_path_buf(),
        None => std::env::current_dir()?.join(name),
    };
    if root.exists() {
        return Err(eyre::eyre!("Path {} already exists", root.display()));
    }

    fs::create_dir_all(root.join("src")).await?;
    fs::create_dir_all(root.join("assets")).await?;
    if kind == "theme" {
        fs::create_dir_all(root.join("templates")).await?;
    }

    let cargo = build_cargo_toml(name, &kind)?;
    fs::write(root.join("Cargo.toml"), cargo).await?;

    let plugin = build_plugin_toml(name, &kind)?;
    fs::write(root.join("Plugin.toml"), plugin).await?;

    let lib = if kind == "theme" {
        TEMPLATE_THEME_LIB
    } else {
        TEMPLATE_HOOK_LIB
    };
    fs::write(root.join("src/lib.rs"), lib).await?;

    fs::write(root.join(".gitignore"), TEMPLATE_GITIGNORE).await?;

    if kind == "theme" {
        fs::write(root.join("templates/index.html"), TEMPLATE_THEME_INDEX).await?;
        fs::write(root.join("templates/article.html"), TEMPLATE_THEME_ARTICLE).await?;
        fs::write(root.join("assets/style.css"), TEMPLATE_STYLE).await?;
    }

    Ok(())
}

async fn plugin_update(name: &str) -> eyre::Result<()> {
    let workspace = Workspace::open(std::env::current_dir()?)
        .await
        .map_err(|_| eyre::eyre!("Not a Thought workspace (Thought.toml missing)"))?;

    let locator_exists = workspace.manifest().plugins().any(|(n, _)| n == name);
    if !locator_exists {
        return Err(eyre::eyre!("Plugin `{name}` not found in Thought.toml"));
    }

    let plugin_dir = workspace
        .cache_dir()
        .join("plugins")
        .join(normalize_name(name));
    let had_cache = fs::metadata(&plugin_dir).await.is_ok();
    let old_head = git_head(&plugin_dir).await;
    let old_hash = wasm_hash(&plugin_dir).await;
    if had_cache {
        fs::remove_dir_all(&plugin_dir).await?;
    }

    // Re-resolve all plugins to ensure dependencies are up-to-date.
    PluginManager::resolve_workspace(&workspace).await?;
    let new_head = git_head(&plugin_dir).await;
    let new_hash = wasm_hash(&plugin_dir).await;

    if let (Some(old), Some(new)) = (old_head.as_ref(), new_head.as_ref()) {
        if old != new {
            println!("Plugin `{name}` updated: git {old} -> {new}");
        } else {
            eprintln!("Plugin `{name}` unchanged (git HEAD stays at {old}).");
        }
    } else if let (Some(old), Some(new)) = (old_hash.as_ref(), new_hash.as_ref()) {
        if old != new {
            println!("Plugin `{name}` updated: artifact hash changed.");
        } else {
            eprintln!("Plugin `{name}` unchanged (artifact hash identical).");
        }
    } else if had_cache {
        println!("Plugin `{name}` refreshed.");
    } else {
        println!("Plugin `{name}` installed.");
    }
    Ok(())
}

fn normalize_name(name: &str) -> String {
    name.chars()
        .map(|ch| match ch {
            '/' | '\\' | ':' => '-',
            _ => ch,
        })
        .collect()
}

async fn wasm_hash(dir: &Path) -> Option<String> {
    let path = dir.join("main.wasm");
    let bytes = fs::read(&path).await.ok()?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    Some(format!("{:x}", hasher.finalize()))
}

async fn git_head(dir: &Path) -> Option<String> {
    if !dir.join(".git").exists() {
        return None;
    }
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .arg("rev-parse")
        .arg("HEAD")
        .output()
        .await
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let head = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if head.is_empty() { None } else { Some(head) }
}

async fn plugin_package(path: &Path, out: Option<&Path>) -> eyre::Result<()> {
    let root = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let manifest_path = root.join("Plugin.toml");
    if !manifest_path.exists() {
        return Err(eyre::eyre!("Plugin.toml not found in {}", root.display()));
    }

    Command::new("cargo")
        .arg("build")
        .arg("--release")
        .arg("--target")
        .arg("wasm32-wasip2")
        .arg("--manifest-path")
        .arg(root.join("Cargo.toml"))
        .status()
        .await
        .map_err(|err: std::io::Error| eyre::eyre!(err))?
        .success()
        .then_some(())
        .ok_or_else(|| eyre::eyre!("cargo build failed"))?;

    let artifact = find_wasm_artifact(&root).await?;
    let wasm_target = root.join("main.wasm");
    fs::copy(&artifact, &wasm_target).await?;

    let out_path = out.map(|p| p.to_path_buf()).unwrap_or_else(|| {
        root.join(format!(
            "{}.tar.gz",
            root.file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("plugin")
        ))
    });
    package_artifact_dir(&root, &out_path)?;
    Ok(())
}

fn build_cargo_toml(name: &str, kind: &str) -> eyre::Result<String> {
    let mut dependencies = BTreeMap::new();
    dependencies.insert(
        "thought-plugin".to_string(),
        Value::Table(
            [(
                "git".to_string(),
                Value::String("https://github.com/lexoliu/thought.git".into()),
            )]
            .into_iter()
            .collect(),
        ),
    );
    if kind == "theme" {
        dependencies.insert("askama".to_string(), Value::String("0.12.1".into()));
    }

    let cargo = CargoToml {
        package: PackageSection {
            name: name.to_string(),
            version: "0.1.0".to_string(),
            edition: "2021".to_string(),
        },
        lib: LibSection {
            crate_type: vec!["cdylib".into(), "rlib".into()],
        },
        dependencies,
    };

    Ok(toml::to_string(&cargo)?)
}

fn build_plugin_toml(name: &str, kind: &str) -> eyre::Result<String> {
    let plugin = PluginToml {
        name: name.to_string(),
        author: whoami::realname(),
        version: "0.1.0".to_string(),
        kind: kind.to_string(),
    };
    Ok(toml::to_string(&plugin)?)
}

fn package_artifact_dir(root: &Path, out: &Path) -> eyre::Result<()> {
    let file = std::fs::File::create(out)?;
    let enc = GzEncoder::new(file, Compression::default());
    let mut tar = Builder::new(enc);

    for entry in ["Plugin.toml", "main.wasm"] {
        let path = root.join(entry);
        if path.exists() {
            tar.append_path_with_name(&path, entry)?;
        }
    }

    let assets = root.join("assets");
    if assets.exists() {
        tar.append_dir_all("assets", assets)?;
    }

    tar.finish()?;
    Ok(())
}

async fn find_wasm_artifact(dir: &Path) -> eyre::Result<PathBuf> {
    let candidates = [
        dir.join("target/wasm32-wasip2/release"),
        dir.join("target/wasm32-wasip2/debug"),
    ];
    for candidate in candidates {
        if let Ok(mut entries) = fs::read_dir(&candidate).await {
            while let Some(entry) = entries.next_entry().await? {
                if entry
                    .file_type()
                    .await
                    .map(|ft| ft.is_file())
                    .unwrap_or(false)
                    && entry.path().extension().and_then(|ext| ext.to_str()) == Some("wasm")
                {
                    return Ok(entry.path());
                }
            }
        }
    }
    Err(eyre::eyre!(
        "Unable to locate built wasm artifact under target/wasm32-wasip2"
    ))
}
