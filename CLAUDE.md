# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Thought is a static site generator written in Rust. It uses WebAssembly for its theme and plugin system, ensuring determinism and portability. Key features include:
- WASI Preview 2 plugins for lifecycle hooks
- Pure Wasm themes for rendering
- Incremental builds with a persistent render cache (redb)
- Multilingual fuzzy search (Tantivy)
- Parallel article processing (tokio + rayon)

## Build Commands

```bash
# Build the CLI
cargo build

# Run the CLI directly
cargo run -- <command>

# Install locally
cargo install --path .

# Run with verbose logging
cargo run -- -v <command>   # DEBUG level
cargo run -- -vv <command>  # TRACE level
```

## Testing

No dedicated test suite exists. Verify changes by running CLI commands against the demo-blog:

```bash
cd demo-blog
cargo run -- generate
cargo run -- serve
cargo run -- search "query"
```

## Linting

The workspace enforces strict clippy lints (pedantic, nursery, cargo levels). Run:

```bash
cargo clippy --all-targets
```

## Architecture

### Core Modules (src/)

- **workspace.rs** - Manages blog directory structure and `Thought.toml` manifest. Entry point for opening/creating blogs.
- **engine.rs** - Rendering coordinator. Orchestrates plugin hooks, theme calls, and output writing.
- **article.rs / category.rs** - Content data models. Parse TOML metadata and Markdown content.
- **plugin.rs / plugin/** - Wasmtime-based plugin runner. Loads and executes Wasm components.
- **cache.rs** - Persistent render cache using redb to skip unchanged articles.
- **search.rs** - Tantivy-based indexing and search. Generates Wasm/JS bundle for client-side search.
- **serve.rs** - Development server (skyzen) with lazy compilation on request.
- **metadata.rs** - TOML schemas for Thought.toml, Article.toml, Category.toml, Plugin.toml.

### Plugin System

Plugins are Wasm components defined by WIT interface (`plugin/wit/plugin.wit`):

- **Themes** (`theme-runtime` world): Pure functions `generate_page` and `generate_index`. No I/O allowed.
- **Hooks** (`hook-runtime` world): Sequential lifecycle hooks `on_pre_render` and `on_post_render`. May perform side effects.

The `thought-plugin` crate (`plugin/`) provides helper traits for plugin authors.

### Content Structure

```
workspace/
├── Thought.toml          # Root config (title, owner, theme, plugins)
├── articles/
│   └── my-article/
│       ├── Article.toml  # Metadata (date, tags, author)
│       ├── article.md    # Default content
│       └── zh-CN.md      # Translation variant
└── build/                # Generated output
```

### CLI Structure (src/cli/)

Entry point: `src/cli/main.rs`. Commands defined via clap derive macros:
- `create` - New workspace
- `article create` - New article
- `generate` - Build static site
- `serve` - Dev server
- `search` - Query search index
- `plugin` - Plugin development helpers
- `translate` - AI-powered translation (OpenRouter)

## Key Dependencies

- **wasmtime** - Wasm runtime for plugins/themes
- **tantivy** - Full-text search engine
- **redb** - Embedded key-value store for cache
- **skyzen** - HTTP server for dev mode
- **pulldown-cmark** - Markdown parsing
