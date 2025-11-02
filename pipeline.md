# Rendering Pipeline

This document describes how `thought` renders the entire blog.

## Step 1: Diff and Scan
If a `/build` directory is found, we can open its `.meta.json` file, which records all page hashes at build time.

## Step 2: Prepare Build Tasks

The index always undergoes a full rebuild, while we only rebuild changed pages. Each page becomes a `task`.

## Step 3: Parse and Process Markdown Natively and in Parallel

At this step, we collect the data required for the build. We parse markdown and metadata, then load essential parts into memory.

## Step 4: Render by Template and Run Plugins

This is where the plugin system takes effect. We provide the following hooks for all plugins:

`on_pre_render` - processes markdown before it is rendered

`on_post_render` - processes HTML after it is rendered

At this stage, we use the current theme. Themes can register `generate_index()` and `generate_page()` hooks.
