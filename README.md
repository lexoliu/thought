# Thought

A blazingly fast static site generator for your blog, written in Rust.

## Features

- **Scaffolding:** Quickly initialize a new blog project.
- **Content Management:** Create new articles and categories with simple commands.
- **Static Site Generation:** Build your entire site into a `build` directory.
- **Live Preview:** A built-in server to preview your site locally.
- **Pure Themes:** Themes are deterministic WebAssembly components that render pages.
- **Lifecycle Plugins:** WASI Preview 2 plugins run sequential hooks around rendering.

## Installation

Ensure you have Rust and Cargo installed. Then, you can install Thought from the source:

```bash
cargo install --path .
```

## Getting Started

1.  **Initialize a new blog:**
    ```bash
    thought init my-awesome-blog
    cd my-awesome-blog
    ```

2.  **Create a new article:**
    ```bash
    thought new "My First Article"
    ```

3.  **Create a new category:**
    ```bash
    thought category guides new "Helpful Guides"
    ```

4.  **Create an article within a category:**
    ```bash
    thought new --category guides "A Helpful Guide"
    ```

5.  **Generate your site:**
    ```bash
    thought generate
    ```
    Your static site will be generated in the `build` directory.

6.  **Serve your site locally:**
    ```bash
    thought serve
    ```
    Your site will be available at `http://127.0.0.1:8080`.

## CLI Commands

- `thought init <name>`: Creates a new blog workspace.
- `thought new <name> [--category <category>]`: Creates a new article.
- `thought category <category> new <name>`: Creates a new category.
- `thought generate [--output <path>]`: Generates the static site.
- `thought serve [--port <port>]`: Serves the generated site.
- `thought clean`: Removes the `build` directory.

## Configuration

The main configuration for your blog is in the `Thought.toml` file. Here you can set the title of your blog, the owner, and the theme to use.

```toml
title = "My Awesome Blog"
owner = "Your Name"
template = "zenflow"
```

## Themes vs. Plugins

Thought distinguishes between **themes** and **plugins** so you can scale presentation and behaviour independently.

- **Themes** are compiled to WebAssembly components (the `theme-runtime` world). They expose pure functions such as `generate_page` and `generate_index`, receive article data, and must return HTML. A theme cannot perform I/O, read clocks, or mutate shared state; the host instantiates it for every render to guarantee determinism and parallelism. You can find the built-in `zenflow` theme under `themes/zenflow`.
- **Plugins** target WASI Preview 2 (`lifecycle-runtime`) and execute sequential lifecycle hooks (`on_pre_render`, `on_post_render`). Plugins may perform side effects such as reading cached data, writing to `/build`, or using time and randomness. They are evaluated in declaration order, so the output of one plugin becomes the input of the next.

When building custom behaviour, choose a theme whenever you only need to transform data into HTML, and reach for a plugin when you need stateful coordination or side effects.
