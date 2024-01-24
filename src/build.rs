use copy_dir::copy_dir;
use liquid::object;
use std::{
    fs::{create_dir, create_dir_all, read_dir, remove_dir_all, File},
    io::{BufWriter, Write},
    path::Path,
};

use crate::{
    article::{Article, ArticlePreview},
    config::Config,
    utils::{workspace, BuildResource, Result},
};

pub fn command(config: Option<&Path>) -> Result<()> {
    let workspace = workspace()?;
    let config = Config::from_file(config.unwrap_or(&workspace.join("Thought.toml")))?;
    let resource = BuildResource::load(config)?;
    let mut articles = Vec::new();
    let output = workspace.join("build");

    let _ = remove_dir_all(&output);

    create_dir(&output)?;

    scan(&mut articles, &workspace.join("articles"), &resource)?;

    articles.sort_by_key(|article| article.metadata.created);

    resource.index_template.render_to(
        &mut BufWriter::new(File::create(workspace.join("build/index.html"))?),
        &object!({"articles":articles,"footer":resource.footer}),
    )?;

    copy_dir(
        resource.template_path()?.join("assets"),
        output.join("assets"),
    )?;

    log::info!("Build product has been saved in {}", output.display());

    Ok(())
}

fn scan(
    articles: &mut Vec<ArticlePreview>,
    current: &Path,
    resource: &BuildResource,
) -> Result<()> {
    for item in read_dir(current)? {
        let item = item?;
        let filetype = item.file_type()?;
        if filetype.is_dir() {
            if let Ok(article) = Article::from_dir(item.path()) {
                articles.push(article.preview());
                build(article, resource)?;
            } else {
                scan(articles, current, resource)?;
            }
        }
    }
    Ok(())
}

fn build(article: Article, resource: &BuildResource) -> Result<()> {
    let workspace = workspace()?;
    log::info!("Building article `{}`", article.title);

    let buf = article.render(resource)?;
    let mut path = workspace.join("build");
    path.extend(article.category);
    path.push(article.name);

    create_dir_all(&path)?;

    BufWriter::new(File::create(path.join("index.html"))?).write_all(buf.as_bytes())?;
    Ok(())
}
