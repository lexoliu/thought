# Thought

A blazingly fast static site generator for your blog, written in Rust.

## Features

- **Scaffolding:** Quickly initialize a new blog project.
- **Content Management:** Create new articles and categories with simple commands.
- **Static Site Generation:** Build your entire site into a `build` directory.
- **Live Preview:** A built-in server to preview your site locally.
- **Theming:** Easily customize the look and feel of your blog.

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

## Theming

Themes are located in the `template` directory. A theme consists of Tera templates for the index page (`index.html`), article page (`article.html`), and a footer (`footer.md`). Static assets can be placed in an `assets` subdirectory within your theme folder.