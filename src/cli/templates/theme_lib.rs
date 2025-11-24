use thought_plugin::{
    askama::Template,
    export_theme,
    helpers::{
        format_rfc3339, index_assets_prefix, index_search_script_path, markdown_to_html,
    },
    Article, ArticlePreview, Theme,
};

pub struct Plugin;

#[derive(Template)]
#[template(path = "article.html")]
struct ArticleTemplate<'a> {
    title: &'a str,
    created: &'a str,
    body: &'a str,
    author: &'a str,
    search_js: &'a str,
    asset_prefix: &'a str,
}

#[derive(Template)]
#[template(path = "index.html")]
struct IndexTemplate<'a> {
    entries: &'a [IndexEntry],
    search_js: &'a str,
    asset_prefix: &'a str,
}

struct IndexEntry {
    title: String,
    href: String,
}

impl Theme for Plugin {
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
}

export_theme!(Plugin);
