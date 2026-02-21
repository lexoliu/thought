#!/usr/bin/env -S rust-script

//! ```cargo
//! [package]
//! edition = "2021"
//!
//! [dependencies]
//! anyhow = "1.0.95"
//! clap = { version = "4.5.20", features = ["derive"] }
//! indicatif = "0.17.9"
//! serde = { version = "1.0.210", features = ["derive"] }
//! serde_json = "1.0.132"
//! time = { version = "0.3.36", features = ["formatting","macros"] }
//! walkdir = "2.5.0"
//! ```

use anyhow::{Context, Result, bail};
use clap::Parser;
use indicatif::{ProgressBar, ProgressStyle};
use serde::Serialize;
use std::{
    env,
    ffi::OsStr,
    fs::{self, File},
    io::{BufWriter, Write},
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};
use time::{Duration as TimeDuration, OffsetDateTime};
use walkdir::WalkDir;

#[derive(Parser, Debug)]
#[command(author, version, about = "Thought vs Hexo benchmark harness")]
struct Args {
    /// Number of articles to synthesize
    #[arg(long, default_value_t = 10_000)]
    articles: usize,

    /// Paragraphs of filler text per article body
    #[arg(long, default_value_t = 12)]
    paragraphs: usize,

    /// Optional Thought binary path (defaults to target/release/thought)
    #[arg(long)]
    thought_bin: Option<PathBuf>,

    /// Optional Hexo CLI binary path (defaults to `hexo`)
    #[arg(long)]
    hexo_bin: Option<PathBuf>,

    /// Additional arguments to pass to `hexo` (appended after `generate --silent`)
    #[arg(long = "hexo-extra-arg", value_name = "ARG")]
    hexo_extra_args: Vec<String>,

    /// Keep the generated workdir instead of deleting it after the run
    #[arg(long)]
    keep_data: bool,

    /// Emit a JSON summary at benchmark/results/latest.json
    #[arg(long)]
    json: bool,

    /// Rebuild the zenflow theme wasm even if a cached artifact exists
    #[arg(long)]
    force_theme_build: bool,
}

#[derive(Serialize)]
struct GeneratorResult {
    name: &'static str,
    duration_ms: u128,
    posts_per_second: f64,
    output_dir: String,
}

#[derive(Serialize)]
struct BenchmarkSummary {
    articles: usize,
    paragraphs: usize,
    results: Vec<GeneratorResult>,
}

struct WorkspaceLayout {
    thought_root: PathBuf,
    hexo_root: PathBuf,
    thought_articles: PathBuf,
    hexo_posts: PathBuf,
}

fn main() -> Result<()> {
    let args = Args::parse();
    if args.articles == 0 {
        bail!("--articles must be greater than zero");
    }

    let repo_root = env::current_dir()
        .context("Unable to read current directory")?
        .canonicalize()
        .context("Failed to canonicalize current directory")?;
    ensure_repo_root(&repo_root)?;

    let bench_root = repo_root.join("benchmark");
    let workdir = bench_root.join("workdir");
    let cache_dir = bench_root.join("cache");
    let theme_cache = cache_dir.join("zenflow-prebuilt");
    let theme_target = cache_dir.join("zenflow-target");

    if workdir.exists() {
        fs::remove_dir_all(&workdir).with_context(|| {
            format!("Failed to remove previous workdir at {}", workdir.display())
        })?;
    }
    fs::create_dir_all(&workdir)
        .with_context(|| format!("Failed to create {}", workdir.display()))?;

    let layout = prepare_workspaces(&workdir, &bench_root)?;
    let prebuilt_theme = ensure_theme_artifact(
        &repo_root,
        &theme_cache,
        &theme_target,
        args.force_theme_build,
    )?;
    let hexo_cache = ensure_hexo_dependencies(&bench_root)?;
    write_thought_manifest(&layout.thought_root, &prebuilt_theme)?;
    write_hexo_config(&layout.hexo_root)?;
    copy_hexo_theme(&bench_root.join("hexo-theme"), &layout.hexo_root)?;
    provision_hexo_dependencies(&hexo_cache, &layout.hexo_root)?;

    println!(
        "Synthesizing {} articles with {} paragraphs each…",
        args.articles, args.paragraphs
    );
    let stats = generate_articles(
        args.articles,
        args.paragraphs,
        &layout.thought_articles,
        &layout.hexo_posts,
    )?;
    println!(
        "Generated {} markdown files (~{:.1} MB of content)",
        args.articles,
        stats.total_bytes as f64 / (1024.0 * 1024.0)
    );

    let thought_bin = resolve_thought_binary(&repo_root, args.thought_bin)?;
    let hexo_bin = args.hexo_bin.unwrap_or_else(|| PathBuf::from("hexo"));

    let thought_duration =
        run_generator(&thought_bin, ["generate"], &layout.thought_root, "Thought")?;
    println!(
        "Thought finished in {:.2}s ({:.1} posts/s)",
        thought_duration.as_secs_f64(),
        throughput(args.articles, thought_duration)
    );

    let mut hexo_args = vec!["generate".to_string(), "--silent".to_string()];
    hexo_args.extend(args.hexo_extra_args.clone());
    let hexo_duration = run_generator(
        &hexo_bin,
        hexo_args.iter().map(|s| s.as_str()),
        &layout.hexo_root,
        "Hexo",
    )?;
    println!(
        "Hexo finished in {:.2}s ({:.1} posts/s)",
        hexo_duration.as_secs_f64(),
        throughput(args.articles, hexo_duration)
    );

    if args.json {
        write_json_summary(
            &bench_root.join("results"),
            BenchmarkSummary {
                articles: args.articles,
                paragraphs: args.paragraphs,
                results: vec![
                    GeneratorResult {
                        name: "thought",
                        duration_ms: thought_duration.as_millis(),
                        posts_per_second: throughput(args.articles, thought_duration),
                        output_dir: layout.thought_root.join("build").display().to_string(),
                    },
                    GeneratorResult {
                        name: "hexo",
                        duration_ms: hexo_duration.as_millis(),
                        posts_per_second: throughput(args.articles, hexo_duration),
                        output_dir: layout.hexo_root.join("public").display().to_string(),
                    },
                ],
            },
        )?;
    }

    if !args.keep_data {
        fs::remove_dir_all(&workdir)?;
    } else {
        println!("Benchmark data kept under {}", workdir.display());
    }
    Ok(())
}

fn ensure_repo_root(root: &Path) -> Result<()> {
    if !root.join("Cargo.toml").exists() || !root.join("benchmark").exists() {
        bail!("Run this script from the repository root");
    }
    Ok(())
}

fn prepare_workspaces(workdir: &Path, bench_root: &Path) -> Result<WorkspaceLayout> {
    let thought_root = workdir.join("thought-workspace");
    let hexo_root = workdir.join("hexo-workspace");
    fs::create_dir_all(&thought_root)?;
    fs::create_dir_all(&hexo_root)?;

    let thought_articles = thought_root.join("articles");
    let hexo_posts = hexo_root.join("source/_posts");

    fs::create_dir_all(&thought_articles)?;
    fs::create_dir_all(&hexo_posts)?;

    // seed category metadata
    let category_toml = thought_articles.join("Category.toml");
    fs::write(
        category_toml,
        r#"created = "2024-01-01T00:00:00Z"
name = "thought-benchmark"
description = "Synthetic benchmark root"
"#,
    )?;

    // ensure theme directory exists
    let themes_dir = hexo_root.join("themes");
    fs::create_dir_all(&themes_dir)?;
    fs::create_dir_all(bench_root.join("results"))?; // create eagerly for later

    Ok(WorkspaceLayout {
        thought_root,
        hexo_root,
        thought_articles,
        hexo_posts,
    })
}

fn write_thought_manifest(workspace: &Path, plugin_dir: &Path) -> Result<()> {
    let manifest = workspace.join("Thought.toml");
    let content = format!(
        r#"name = "thought-benchmark"
description = "Synthetic benchmark workspace"
owner = "Benchmark Bot"

[plugins.zenflow]
path = "{}"
"#,
        plugin_dir.display()
    );
    fs::write(manifest, content)?;
    Ok(())
}

fn write_hexo_config(workspace: &Path) -> Result<()> {
    let config = workspace.join("_config.yml");
    let contents = r#"title: Thought Benchmark
subtitle:
description: Synthetic benchmark workspace
theme: thought-benchmark
url: https://example.com
root: /
per_page: 0
language: en

search:
  path: search.json
  field: post
  format: html
"#;
    fs::write(config, contents)?;
    Ok(())
}

fn copy_hexo_theme(theme_src: &Path, workspace: &Path) -> Result<()> {
    let target = workspace.join("themes/thought-benchmark");
    if target.exists() {
        fs::remove_dir_all(&target)?;
    }
    copy_tree(theme_src, &target)?;
    Ok(())
}

fn copy_tree(src: &Path, dst: &Path) -> Result<()> {
    for entry in WalkDir::new(src) {
        let entry = entry?;
        let relative = entry.path().strip_prefix(src).unwrap();
        let target = dst.join(relative);
        if entry.file_type().is_dir() {
            fs::create_dir_all(&target)?;
        } else {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

fn ensure_hexo_dependencies(bench_root: &Path) -> Result<PathBuf> {
    let template = bench_root.join("hexo-package");
    let template_pkg = template.join("package.json");
    if !template_pkg.exists() {
        bail!(
            "Missing Hexo package template at {}",
            template_pkg.display()
        );
    }

    let cache = bench_root.join("cache/hexo-node");
    fs::create_dir_all(&cache)?;

    let desired_pkg = fs::read_to_string(&template_pkg)?;
    let cache_pkg = cache.join("package.json");
    let mut needs_install = true;
    if cache_pkg.exists() {
        let current = fs::read_to_string(&cache_pkg)?;
        if current == desired_pkg && cache.join("node_modules").exists() {
            needs_install = false;
        }
    }
    fs::write(&cache_pkg, desired_pkg)?;

    let cache_lock = cache.join("package-lock.json");
    if !template.join("package-lock.json").exists() && cache_lock.exists() && needs_install {
        let _ = fs::remove_file(&cache_lock);
    }

    if needs_install {
        println!("Installing Hexo npm dependencies (first run only)…");
        if cache.join("node_modules").exists() {
            fs::remove_dir_all(cache.join("node_modules"))?;
        }
        let status = Command::new("npm")
            .arg("install")
            .arg("--production")
            .arg("--silent")
            .current_dir(&cache)
            .status()
            .context("Failed to run npm install for Hexo dependencies")?;
        if !status.success() {
            bail!("npm install for Hexo dependencies exited with {status}");
        }
    }
    if !cache.join("node_modules").exists() {
        bail!(
            "npm install did not create node_modules in {}",
            cache.display()
        );
    }
    Ok(cache)
}

fn provision_hexo_dependencies(cache: &Path, workspace: &Path) -> Result<()> {
    let pkg_src = cache.join("package.json");
    fs::copy(&pkg_src, workspace.join("package.json"))?;
    let lock_src = cache.join("package-lock.json");
    if lock_src.exists() {
        fs::copy(&lock_src, workspace.join("package-lock.json"))?;
    }
    let node_modules_src = cache.join("node_modules");
    let node_modules_dst = workspace.join("node_modules");
    if node_modules_dst.exists() {
        fs::remove_dir_all(&node_modules_dst)?;
    }
    copy_tree(&node_modules_src, &node_modules_dst)?;
    Ok(())
}

struct GenerationStats {
    total_bytes: usize,
}

fn generate_articles(
    total: usize,
    paragraphs: usize,
    thought_articles: &Path,
    hexo_posts: &Path,
) -> Result<GenerationStats> {
    let pb = ProgressBar::new(total as u64);
    pb.set_style(
        ProgressStyle::with_template("{spinner:.green} generating articles… {pos}/{len}").unwrap(),
    );
    let mut total_bytes = 0usize;
    for idx in 0..total {
        let slug = format!("post-{idx:05}");
        let title = format!("Benchmark Article #{idx:05}");
        let created = synthesized_timestamp(idx);
        let description = format!("Synthetic benchmark article #{idx:05}");
        let body = build_article_body(idx, paragraphs);
        let article_md = format!("# {title}\n\n{body}\n");
        total_bytes += article_md.len();

        let article_dir = thought_articles.join(&slug);
        fs::create_dir_all(&article_dir)?;
        let mut meta = BufWriter::new(File::create(article_dir.join("Article.toml"))?);
        writeln!(meta, r#"created = "{created}""#)?;
        writeln!(meta, "tags = []")?;
        writeln!(meta, r#"author = "Benchmark Bot""#)?;
        writeln!(meta, r#"description = "{description}""#)?;
        meta.flush()?;

        let mut article_file = BufWriter::new(File::create(article_dir.join("article.md"))?);
        article_file.write_all(article_md.as_bytes())?;
        article_file.flush()?;

        let mut hexo_file = BufWriter::new(File::create(hexo_posts.join(format!("{slug}.md")))?);
        writeln!(hexo_file, "---")?;
        writeln!(hexo_file, r#"title: "{title}""#)?;
        writeln!(hexo_file, "date: {created}")?;
        writeln!(hexo_file, r#"description: "{description}""#)?;
        writeln!(hexo_file, "---\n")?;
        hexo_file.write_all(article_md.as_bytes())?;
        hexo_file.flush()?;

        pb.inc(1);
    }
    pb.finish_with_message("articles ready");
    Ok(GenerationStats { total_bytes })
}

fn synthesized_timestamp(idx: usize) -> String {
    let base = OffsetDateTime::from_unix_timestamp(1_700_000_000).unwrap();
    let dt = base + TimeDuration::seconds(idx as i64);
    dt.format(&time::format_description::well_known::Rfc3339)
        .unwrap()
}

fn build_article_body(idx: usize, paragraphs: usize) -> String {
    const FILLER: &str = "Lorem ipsum dolor sit amet, consectetur adipiscing elit. Integer suscipit ante sit amet tortor vulputate, vitae imperdiet justo malesuada. Suspendisse potenti. ";
    let mut body = String::with_capacity(paragraphs * FILLER.len() * 2);
    for p in 0..paragraphs {
        body.push_str(&format!(
            "## Section {p}\n\n{FILLER}Reference #{idx}-{p}. {FILLER}\n\n"
        ));
    }
    body
}

fn resolve_thought_binary(repo_root: &Path, override_path: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(bin) = override_path {
        return Ok(bin);
    }
    let bin_name = format!("thought{}", std::env::consts::EXE_SUFFIX);
    let candidate = repo_root.join("target").join("release").join(bin_name);
    if candidate.exists() {
        return Ok(candidate);
    }
    bail!(
        "Thought binary not found at {}. Pass --thought-bin or run `cargo build --release` first.",
        candidate.display()
    );
}

fn ensure_theme_artifact(
    repo_root: &Path,
    cache_dir: &Path,
    target_dir: &Path,
    force: bool,
) -> Result<PathBuf> {
    let theme_src = locate_zenflow_theme(repo_root)?;
    fs::create_dir_all(cache_dir)?;
    fs::create_dir_all(target_dir)?;

    let plugin_manifest = theme_src.join("Plugin.toml");
    let target_manifest = cache_dir.join("Plugin.toml");
    fs::copy(&plugin_manifest, &target_manifest)?;

    let wasm_dest = cache_dir.join("main.wasm");
    if !wasm_dest.exists() || force {
        println!("Building zenflow theme to warm up WASM artifact…");
        let status = Command::new("cargo")
            .arg("build")
            .arg("--release")
            .arg("--target")
            .arg("wasm32-wasip2")
            .current_dir(&theme_src)
            .env("CARGO_TARGET_DIR", target_dir)
            .status()
            .context("Failed to launch cargo for zenflow build")?;
        if !status.success() {
            bail!("Building zenflow theme failed (exit code {status})");
        }
        let built = find_wasm_artifact(target_dir)?;
        fs::copy(&built, &wasm_dest)?;
    }
    Ok(cache_dir.to_path_buf())
}

fn find_wasm_artifact(target_dir: &Path) -> Result<PathBuf> {
    let release_dir = target_dir.join("wasm32-wasip2/release");
    if !release_dir.exists() {
        bail!("No wasm artifacts found under {}", release_dir.display());
    }
    for entry in fs::read_dir(&release_dir)? {
        let entry = entry?;
        if entry
            .path()
            .extension()
            .and_then(OsStr::to_str)
            .map(|ext| ext == "wasm")
            .unwrap_or(false)
        {
            return Ok(entry.path());
        }
    }
    bail!(
        "Unable to locate .wasm under {}. Ensure zenflow builds successfully.",
        release_dir.display()
    );
}

fn locate_zenflow_theme(repo_root: &Path) -> Result<PathBuf> {
    let defaults = [
        repo_root.join("themes/zenflow"),
        repo_root.join("demo-blog/.thought/plugins/zenflow"),
    ];
    for candidate in defaults {
        if candidate.join("Plugin.toml").exists() {
            return Ok(candidate);
        }
    }

    for entry in WalkDir::new(repo_root)
        .max_depth(6)
        .follow_links(false)
        .into_iter()
        .filter_map(std::result::Result::ok)
    {
        if !entry.file_type().is_dir() {
            continue;
        }
        let path = entry.path();
        if contains_blacklisted_component(path) {
            continue;
        }
        if path.file_name().and_then(OsStr::to_str) != Some("zenflow") {
            continue;
        }
        if path.join("Plugin.toml").exists() {
            return Ok(path.to_path_buf());
        }
    }

    bail!(
        "Unable to locate zenflow theme directory; ensure the repository contains themes/zenflow or demo-blog/.thought/plugins/zenflow."
    );
}

fn contains_blacklisted_component(path: &Path) -> bool {
    path.components().any(|component| {
        matches!(
            component.as_os_str().to_str(),
            Some(".git") | Some("target") | Some("node_modules")
        )
    })
}

fn run_generator<'a>(
    binary: &Path,
    args: impl IntoIterator<Item = &'a str>,
    cwd: &Path,
    label: &str,
) -> Result<Duration> {
    let start = Instant::now();
    let status = Command::new(binary)
        .args(args)
        .current_dir(cwd)
        .status()
        .with_context(|| format!("Failed to start {label}"))?;
    if !status.success() {
        bail!("{label} exited with {}", status);
    }
    Ok(start.elapsed())
}

fn throughput(articles: usize, duration: Duration) -> f64 {
    articles as f64 / duration.as_secs_f64()
}

fn write_json_summary(results_dir: &Path, summary: BenchmarkSummary) -> Result<()> {
    fs::create_dir_all(results_dir)?;
    let payload = serde_json::to_vec_pretty(&summary)?;
    let path = results_dir.join("latest.json");
    fs::write(&path, payload)?;
    println!("Wrote {}", path.display());
    Ok(())
}
