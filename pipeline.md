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

- **Lifecycle hooks** run one-at-a-time in declaration order. Each hook receives the article returned by the previous hook. `on_pre_render` can mutate the article model (e.g. enrich metadata), while `on_post_render` mutates the generated HTML. Hooks are *pure* WebAssembly components: they cannot touch I/O, clocks, or randomness. We still instantiate them with the WASI Preview 2 command world so they link cleanly, but the host does not preopen any directories or provide side-effecting capabilities.
- **Theme rendering** happens between the two hook invocations. Themes also implement pure functions (`generate_page`, `generate_index`) that return HTML purely from the provided data.

This purity requirement means every plugin invocation is deterministic and side-effect free, enabling aggressive caching and embarrassingly parallel rendering. Because no shared state leaks into the guest, we can instantiate themes and hooks on-demand across threads without worrying about `!Sync` data inside Wasi contexts. WASI Preview 2 remains our ABI, but it is treated strictly as a type/ABI surface rather than an escape hatch into host resources.
