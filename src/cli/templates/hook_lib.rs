use thought_plugin::{export_hook, Article, Hook};

pub struct Plugin;

impl Hook for Plugin {
    fn on_pre_render(article: Article) -> Result<Article, String> {
        Ok(article)
    }

    fn on_post_render(article: Article, html: String) -> Result<String, String> {
        let _ = article;
        Ok(html)
    }
}

export_hook!(Plugin);
