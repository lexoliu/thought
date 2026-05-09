use std::{collections::HashMap, path::{Path, PathBuf}};

use ignore::WalkBuilder;

use crate::{
    article::{ArticleSource, MdFile},
    io_backend,
};

/// Paths collected for a single article directory.
#[derive(Debug)]
pub struct ArticleDirPaths {
    /// Path segments relative to the articles root (e.g. `["tech", "my-post"]`).
    pub segments: Vec<String>,
    /// Absolute path to `Article.toml`.
    pub metadata_path: PathBuf,
    /// `(filename, absolute_path)` for each `.md` file.
    pub md_paths: Vec<(String, PathBuf)>,
}

/// Paths collected for a single category directory.
#[derive(Debug)]
pub struct CategoryPaths {
    /// Path segments relative to the articles root.
    pub segments: Vec<String>,
    /// Absolute path to `Category.toml`.
    pub metadata_path: PathBuf,
}

/// Result of scanning the workspace articles directory.
#[derive(Debug)]
pub struct ScanResult {
    pub article_dirs: Vec<ArticleDirPaths>,
    pub category_dirs: Vec<CategoryPaths>,
}

/// Walk the articles directory using the `ignore` crate, collecting paths
/// for articles and categories without reading any file content.
pub fn scan_workspace_files(articles_dir: &Path) -> ScanResult {
    let mut article_dirs = Vec::new();
    let mut category_dirs = Vec::new();

    // Collect all directories that contain Article.toml or Category.toml
    let walker = WalkBuilder::new(articles_dir)
        .hidden(true) // skip hidden files/dirs
        .build();

    for entry in walker.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let segments = match path.strip_prefix(articles_dir) {
            Ok(rel) if rel.as_os_str().is_empty() => Vec::new(),
            Ok(rel) => rel
                .components()
                .filter_map(|c| c.as_os_str().to_str().map(String::from))
                .collect(),
            Err(_) => continue,
        };

        let article_toml = path.join("Article.toml");
        if article_toml.exists() {
            // Collect all .md files in this directory
            let md_paths: Vec<(String, PathBuf)> = std::fs::read_dir(path)
                .into_iter()
                .flatten()
                .filter_map(Result::ok)
                .filter(|e| {
                    e.path().extension().and_then(|ext| ext.to_str()) == Some("md")
                        && e.file_type().map(|ft| ft.is_file()).unwrap_or(false)
                })
                .map(|e| {
                    let filename = e.file_name().to_string_lossy().into_owned();
                    (filename, e.path())
                })
                .collect();

            if !md_paths.is_empty() {
                article_dirs.push(ArticleDirPaths {
                    segments,
                    metadata_path: article_toml,
                    md_paths,
                });
            }
            continue; // article dirs don't also count as categories
        }

        let category_toml = path.join("Category.toml");
        if category_toml.exists() {
            category_dirs.push(CategoryPaths {
                segments,
                metadata_path: category_toml,
            });
        }
    }

    ScanResult {
        article_dirs,
        category_dirs,
    }
}

/// Raw category bytes: `(segments, toml_bytes)`.
pub type CategoryRawBytes = (Vec<String>, Vec<u8>);

/// Batch-read all scanned paths via `io_backend` and assemble into
/// `ArticleSource` / category raw bytes.
pub fn build_sources(
    scan: &ScanResult,
) -> std::io::Result<(Vec<ArticleSource>, Vec<CategoryRawBytes>)> {
    // Flatten all paths into a single batch for maximum I/O throughput
    let mut all_paths: Vec<PathBuf> = Vec::new();

    // Track which index ranges belong to which article/category
    // For each article: (start_of_metadata, start_of_mds, count_of_mds)
    struct ArticleRange {
        metadata_idx: usize,
        md_start: usize,
        md_count: usize,
    }

    let mut article_ranges: Vec<ArticleRange> = Vec::with_capacity(scan.article_dirs.len());
    let mut category_indices: Vec<usize> = Vec::with_capacity(scan.category_dirs.len());

    for article_dir in &scan.article_dirs {
        let metadata_idx = all_paths.len();
        all_paths.push(article_dir.metadata_path.clone());
        let md_start = all_paths.len();
        for (_, md_path) in &article_dir.md_paths {
            all_paths.push(md_path.clone());
        }
        article_ranges.push(ArticleRange {
            metadata_idx,
            md_start,
            md_count: article_dir.md_paths.len(),
        });
    }

    for cat in &scan.category_dirs {
        category_indices.push(all_paths.len());
        all_paths.push(cat.metadata_path.clone());
    }

    // Single batched I/O call — io_uring on Linux, rayon+std::fs on macOS
    let read_results = io_backend::batch_read_files(&all_paths)?;

    // Index by path for O(1) lookup
    let by_path: HashMap<&Path, &[u8]> = read_results
        .iter()
        .map(|(p, data)| (p.as_path(), data.as_slice()))
        .collect();

    // Assemble ArticleSources
    let mut article_sources = Vec::with_capacity(scan.article_dirs.len());
    for (i, article_dir) in scan.article_dirs.iter().enumerate() {
        let range = &article_ranges[i];
        let metadata_bytes = by_path[all_paths[range.metadata_idx].as_path()].to_vec();

        let md_files: Vec<MdFile> = (0..range.md_count)
            .map(|j| {
                let path = &all_paths[range.md_start + j];
                let filename = article_dir.md_paths[j].0.clone();
                let content = by_path[path.as_path()].to_vec();
                MdFile { filename, content }
            })
            .collect();

        article_sources.push(ArticleSource {
            segments: article_dir.segments.clone(),
            metadata_bytes,
            md_files,
        });
    }

    // Assemble category bytes
    let mut category_bytes = Vec::with_capacity(scan.category_dirs.len());
    for (i, cat) in scan.category_dirs.iter().enumerate() {
        let idx = category_indices[i];
        let bytes = by_path[all_paths[idx].as_path()].to_vec();
        category_bytes.push((cat.segments.clone(), bytes));
    }

    Ok((article_sources, category_bytes))
}
