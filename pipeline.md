# Rendering Pipeline

This document describes how `thought` renders the entire blog.

## Step 1: Diff and Scan
If a `/build` directory is found, we can open its `.meta.json` file, which records all page hashes at build time.

## Step 2: Prepare Build Tasks

The index always undergoes a full rebuild, while we only rebuild changed pages. Each page becomes a `task`.

## Step 3: Parse and Process Markdown Natively and in Parallel

At this step, we collect the data required for the build. We parse markdown and metadata, then load essential parts into memory.

## Step 4: Plugin Lifecycle and Theme Rendering

Rendering is split into two distinct stages:

- **Lifecycle plugins** run one-at-a-time in declaration order. Each plugin receives the article produced by the previous stage. `on_pre_render` can mutate the article model (e.g. enrich metadata), while `on_post_render` mutates the generated HTML.
- **Theme rendering** happens between the two plugin hooks. Themes expose pure functions (`generate_page`, `generate_index`) that return HTML without touching I/O, clocks, or randomness. We instantiate themes fresh for every render so page generation remains deterministic and parallelisable.

Because plugins can rely on WASI Preview 2 capabilities, they may interact with the filesystem namespaces (`/tmp`, `/cache`, `/build`) or clock APIs. Themes, by contrast, are pure and side-effect free.
