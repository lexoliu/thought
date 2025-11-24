# Creating a Custom Theme for Thought: A Step-by-Step Guide

Welcome to the world of Thought theming! Thought uses a powerful, flexible plugin system that lets you control every pixel of your site's appearance. Themes are packaged as self-contained WebAssembly (Wasm) artifacts, meaning they are portable, secure, and don't require users to have a Rust toolchain installed.

This guide will walk you through building a complete, search-enabled theme from scratch. We'll start with the basics and progressively add features, explaining the "why" behind each step. By the end, you'll have a distributable theme artifact ready to use.

## Part 1: Scaffolding Your Theme

The quickest way to start is by letting Thought generate a starter theme for you. Open your terminal and run:

```bash
thought plugin create my-awesome-theme --kind theme
cd my-awesome-theme
```

This creates a new directory `my-awesome-theme` with all the necessary files to get started:

-   `Plugin.toml`: Metadata for your theme, like its name and author.
-   `Cargo.toml`: The Rust package manifest, pre-configured for a Wasm component.
-   `src/lib.rs`: The heart of your theme's logic.
-   `templates/`: Directory for your HTML templates.
    -   `index.html`: The template for your homepage.
    -   `article.html`: The template for individual articles.
-   `assets/`: For static files like CSS, JavaScript, or images.
    -   `style.css`: A minimal stylesheet.
-   `.gitignore`: Sensible defaults for a Thought plugin.

This scaffold is a fully functional theme. You can build and package it right away!

## Part 2: The `Theme` Trait and Your Logic

Open up `src/lib.rs`. This is where you'll define how your site is rendered. The key component is the `Theme` trait, provided by the `thought-plugin` crate.

```rust
pub trait Theme {
    fn generate_page(article: Article) -> String;
    fn generate_index(articles: Vec<ArticlePreview>) -> String;
}
```

You just need to implement this trait for your `Plugin` struct:

-   `generate_page`: Takes a single `Article` and returns the full HTML for that page.
-   `generate_index`: Takes a list of `ArticlePreview`s and returns the HTML for your site's index page.

The scaffold already provides a default implementation, which we'll dissect next.

## Part 3: Templating with Askama

Thought themes use the [Askama](https://djc.github.io/askama/) template engine by default. It compiles your templates into efficient Rust code, giving you great performance and type safety.

The concept is simple:
1.  Define a Rust `struct` to hold the data for your template.
2.  Use the `#[derive(Template)]` macro on it.
3.  Link it to an HTML file in your `templates/` directory.

The scaffold defines two such structs in `src/lib.rs`:

```rust
#[derive(Template)]
#[template(path = "article.html")] // <-- Links to templates/article.html
struct ArticleTemplate<'a> {
    title: &'a str,
    created: &'a str,
    body: &'a str,
    author: &'a str,
    search_js: &'a str,
    asset_prefix: &'a str,
}

#[derive(Template)]
#[template(path = "index.html")] // <-- Links to templates/index.html
struct IndexTemplate<'a> {
    entries: &'a [IndexEntry],
    search_js: &'a str,
    asset_prefix: &'a str,
}
```

Any field in these structs becomes a variable you can use in your HTML templates.

## Part 4: Rendering a Single Article

Let's look at the `generate_page` implementation and the corresponding `article.html` template.

### The Rust Code (`src/lib.rs`)

Here’s the default implementation:

```rust
// In: impl Theme for Plugin

fn generate_page(article: Article) -> String {
    let created = format_rfc3339(article.metadata().created());

    ArticleTemplate {
        title: article.title(),
        created: &created,
        body: markdown_to_html(article.content()).as_str(),
        author: article.metadata().author(),
        search_js: &article.search_script_path(),
        asset_prefix: &article.assets_prefix(),
    }
    .render()
    .expect("failed to render article template")
}
```

Here's what's happening:
1.  We get the article's creation date and format it. `article.metadata().created()` gives us a `OffsetDateTime`.
2.  We instantiate our `ArticleTemplate` struct, filling its fields with data from the `article` object.
3.  We use helpers from `thought_plugin::helpers`:
    -   `markdown_to_html` converts the article's Markdown content into HTML.
    -   `article.search_script_path()` generates the correct relative path to Thought's built-in search JavaScript based on where the article lives in your category tree.
    -   `article.assets_prefix()` generates the correct relative path prefix for your static assets (like CSS). Using these helpers keeps links working even on deeply nested pages.
4.  Finally, `.render()` generates the HTML string.

### The HTML Template (`templates/article.html`)

The template can now use the data we passed:

```html
<!DOCTYPE html>
<html>
  <head>
    <meta charset="UTF-8">
    <!-- Use asset_prefix for the stylesheet path -->
    <link rel="stylesheet" href="{{ asset_prefix }}assets/style.css">
    <title>{{ title }}</title>
  </head>
  <body>
    <!-- And here for the index link -->
    <a href="{{ asset_prefix }}index.html">← Back</a>
    <h1>{{ title }}</h1>
    <p>By {{ author }} · {{ created }}</p>
    <div>{{ body | safe }}</div>
    <script src="{{ search_js }}" defer></script>
  </body>
</html>
```

Notice how `{{ title }}`, `{{ author }}`, `{{ created }}`, and `{{ search_js }}` directly correspond to the fields of `ArticleTemplate`. The `{{ body | safe }}` syntax tells Askama that the `body` variable contains HTML that should not be escaped.

## Part 5: Rendering the Index Page

The index page follows the same pattern.

### The Rust Code (`src/lib.rs`)

```rust
// In: impl Theme for Plugin

fn generate_index(articles: Vec<ArticlePreview>) -> String {
    let entries = articles
        .into_iter()
        .map(|article| IndexEntry {
            title: article.title().to_string(),
            href: article.output_file(),
        })
        .collect::<Vec<_>>();

    IndexTemplate {
        entries: &entries,
        search_js: index_search_script_path(),
        asset_prefix: index_assets_prefix(),
    }
    .render()
    .expect("failed to render index template")
}
```

1.  It iterates over the list of `ArticlePreview`s.
2.  For each preview, it creates an `IndexEntry` struct containing the title and a link (`href`). The `output_file()` method on `ArticlePreview` generates the path to the final HTML file (e.g., `blog/my-post.html`).
3.  It renders the `IndexTemplate`, passing the list of entries. We use `index_search_script_path()` and `index_assets_prefix()` so the root page always links to the right assets.

### The HTML Template (`templates/index.html`)

The index template loops over the entries:

```html
<!DOCTYPE html>
<html>
  <head>
    <meta charset="UTF-8">
    <link rel="stylesheet" href="{{ asset_prefix }}assets/style.css">
    <title>{{ entries | length }} articles</title>
  </head>
  <body>
    <h1>{{ entries | length }} article(s)</h1>
    <ul>
    {% for entry in entries %}
      <li><a href="{{ entry.href }}">{{ entry.title }}</a></li>
    {% endfor %}
    {% if entries | length == 0 %}
      <li>No content yet.</li>
    {% endif %}
    </ul>
    <script src="{{ search_js }}" defer></script>
  </body>
</html>
```

Askama's `{% for %}` loop makes it easy to generate the list of articles.

## Part 6: Building and Packaging Your Theme

Once you're happy with your theme, you can build and package it into a distributable artifact. From your theme's directory, run:

```bash
thought plugin package --path . --out my-awesome-theme.tar.gz
```

This command does two things:
1.  **Builds** your Rust code into a WebAssembly component (`main.wasm`).
2.  **Packages** the essential files into a gzipped tarball (`.tar.gz`).

The final artifact contains only:
-   `main.wasm` (your compiled theme)
-   `Plugin.toml` (its manifest)
-   `assets/` (your static files)

Your source code and templates are **not** included in the final artifact. This makes your theme a portable, lightweight, and secure binary.

## Part 7: Using Your Theme

To use your new theme in a Thought project, update your `Thought.toml` file:

```toml
[plugins.my-awesome-theme]
# You can host the artifact and point to its URL
url = "https://example.com/my-awesome-theme.tar.gz"

# Or, for local development, use a path
# path = "../relative/path/to/my-awesome-theme"
```

Using a `path` locator is great for development, as Thought will automatically rebuild your theme when it changes. For production, the `url` locator is preferred.

Now, just run the development server to see your theme in action:

```bash
thought serve
```

Navigate to `http://127.0.0.1:2006` (or whichever port it chooses), and you should see your site rendered with your awesome new theme!

## Best Practices & Debugging

-   **Always Use Path Helpers**: Don't hardcode paths like `../../assets/style.css`. Use `article.assets_prefix()` (or `article.assets_path("style.css")`) and `article.search_script_path()` so links work everywhere. For the index page, rely on `index_assets_prefix()` / `index_assets_path()` and `index_search_script_path()`.
-   **Reserved `assets` Category**: Do not name an article category `assets`. This name is reserved for serving static files.
-   **Stale Wasm?**: If you are using a `path` locator, Thought rebuilds your plugin on each run to ensure you're always using the latest version. This is intentional. If you're using a `url` locator, make sure you've uploaded the newest artifact.
-   **Broken Styles/Scripts?**: Check that you are passing `asset_prefix` (from `article.assets_prefix()` or `index_assets_prefix()`) to your templates and using it correctly for all `href` and `src` attributes so nested pages resolve correctly.

Happy theming!
