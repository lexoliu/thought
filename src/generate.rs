use std::{
    fs::{create_dir_all, remove_dir_all},
    path::Path,
};

use crate::{
    article::Article,
    utils::{create_file, render_markdown},
    Error, Result, Workspace,
};
use copy_dir::copy_dir;
use itertools::Itertools;
use tera::{Context, Tera};

pub(crate) mod context {
    use serde::Serialize;

    use crate::{article::ArticlePreview, Workspace};
    #[derive(Debug, Serialize)]

    pub struct Index<'a> {
        site: Site<'a>,
        articles: &'a [ArticlePreview],
    }

    impl<'a> Index<'a> {
        pub fn new(site: Site<'a>, articles: &'a [ArticlePreview]) -> Self {
            Self { site, articles }
        }
    }
    #[derive(Debug, Clone, Serialize)]

    pub struct Article<'a> {
        site: Site<'a>,
        #[serde(flatten)]
        article: &'a crate::article::Article,
        root: String,
    }

    impl<'a> Article<'a> {
        pub fn new(article: &'a crate::article::Article, site: Site<'a>) -> Self {
            Self {
                site,
                article,
                root: "../".repeat(article.category().path().len() + 1),
            }
        }
    }

    #[derive(Debug, Clone, Copy, Serialize)]
    pub struct Site<'a> {
        title: &'a str,
        owner: &'a str,
        footer: &'a str,
    }

    impl<'a> Site<'a> {
        pub fn new(workspace: &'a Workspace, footer: &'a str) -> Self {
            let config = workspace.config();
            Self {
                title: config.title(),
                owner: config.owner(),
                footer,
            }
        }

        pub fn set_footer(&mut self, footer: &'a str) {
            self.footer = footer;
        }
    }
}

pub(crate) fn generate_index(
    engine: &Tera,
    workspace: &Workspace,
    site_context: context::Site<'_>,
) -> Result<String> {
    let mut articles: Vec<_> = workspace.all_articles()?.try_collect()?;

    articles.sort_by_key(|article| article.metadata().created());

    let index = engine.render(
        "index",
        &Context::from_serialize(context::Index::new(site_context, &articles)).unwrap(),
    )?;
    Ok(index)
}

pub(crate) fn generate_article(
    engine: &Tera,
    site_context: context::Site<'_>,
    article: &Article,
) -> Result<String> {
    let article_context = context::Article::new(article, site_context);

    Ok(engine.render(
        "article",
        &Context::from_serialize(article_context).unwrap(),
    )?)
}

pub(crate) fn generate_footer(engine: &Tera, site_context: context::Site<'_>) -> Result<String> {
    Ok(render_markdown(engine.render(
        "footer",
        &Context::from_serialize(site_context).unwrap(),
    )?))
}

pub(crate) fn template_engine(workspace: &Workspace) -> Result<Tera> {
    if workspace.config().template() == "[INSTALL ONE]" {
        return Err(Error::NeedInstallTemplate);
    }

    if !workspace.template_path().exists() {
        return Err(Error::TemplateNotFound {
            name: workspace.config().template().into(),
        });
    }

    let mut engine = Tera::default();
    engine.add_template_file(workspace.template_path().join("index.html"), Some("index"))?;

    engine.add_template_file(
        workspace.template_path().join("article.html"),
        Some("article"),
    )?;

    engine.add_template_file(workspace.path().join("footer.md"), Some("footer"))?;

    Ok(engine)
}

pub(crate) fn generate(workspace: &Workspace, output: &Path) -> Result<()> {
    let engine = template_engine(workspace)?;

    let mut articles: Vec<_> = workspace.all_articles()?.try_collect()?;

    articles.sort_by_key(|article| article.metadata().created());

    let generate_path = workspace.generate_path();

    if generate_path.exists() {
        remove_dir_all(&generate_path)?;
    }

    create_dir_all(&generate_path)?;

    copy_dir(
        workspace.template_path().join("assets"),
        generate_path.join("assets"),
    )?;

    let mut site_context = context::Site::new(workspace, "");
    let footer = generate_footer(&engine, site_context)?;

    site_context.set_footer(&footer);

    create_file(
        output.join("index.html"),
        generate_index(&engine, workspace, site_context)?,
    )?;

    for article in articles {
        let article = article.detail()?;

        let mut path = output.to_owned();
        path.extend(article.category().path());
        path.push(article.name());

        create_dir_all(&path)?;

        create_file(
            path.join("index.html"),
            generate_article(&engine, site_context, &article)?,
        )?;
    }

    Ok(())
}
