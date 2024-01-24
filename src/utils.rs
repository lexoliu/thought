use liquid::Template;
use pulldown_cmark::{html::push_html, Event};
use std::{
    env::current_dir,
    fs::File,
    io::{BufReader, Read},
    path::{Path, PathBuf},
    sync::OnceLock,
};
use thiserror::Error;

use crate::config::Config;
pub type Result<T> = std::result::Result<T, Error>;
#[derive(Debug, Error)]
pub enum Error {
    #[error("Inner error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Cannot parse template: {0}")]
    Templte(#[from] liquid::Error),
    #[error("Workspace already exists")]
    WorkspaceAlreadyExists,
    #[error("Workspace not found")]
    WorkspaceNotFound,
    #[error("Post already exists")]
    PostAlreadyExists,
    #[error("Invalid config: {0}")]
    InvalidConfig(toml::de::Error),
    #[error("Invalid metadata: {0}")]
    InvalidMetadata(toml::de::Error),
    #[error("The name of category must be legal UTF-8 string")]
    IllegalCategoryName,
}

static WORKSPACE: OnceLock<PathBuf> = OnceLock::new();

pub fn workspace() -> Result<&'static Path> {
    let path = get_workspace()?;
    Ok(WORKSPACE.get_or_init(|| path)) // if `get_or_try_init` stabilizes, we will turn to it.
}

fn get_workspace() -> Result<PathBuf> {
    let mut path = current_dir()?;

    loop {
        let file = File::open(path.join("Thought.toml"));
        if file.is_ok() {
            break Ok(path);
        } else if let Some(parent) = path.parent() {
            path = parent.to_owned();
        } else {
            break Err(Error::WorkspaceNotFound);
        }
    }
}

pub fn read_to_string(path: impl AsRef<Path>) -> std::io::Result<String> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut buf = String::new();
    reader.read_to_string(&mut buf)?;
    Ok(buf)
}

pub fn to_html<'a>(iter: impl IntoIterator<Item = Event<'a>>) -> String {
    let mut html = String::new();
    push_html(&mut html, iter.into_iter());
    html
}

pub struct BuildResource {
    pub article_template: Template,
    pub index_template: Template,
    pub footer: String,
    pub config: Config,
}

pub fn render_markdown(markdown: impl AsRef<str>) -> String {
    let parser = pulldown_cmark::Parser::new(markdown.as_ref());
    to_html(parser)
}

impl BuildResource {
    // Load from workspace
    pub fn load(config: Config) -> Result<Self> {
        let workspace = workspace()?;
        let index_template = read_to_string(
            workspace
                .join("template")
                .join(&config.template)
                .join("index.html"),
        )?;

        let article_template = read_to_string(
            workspace
                .join("template")
                .join(&config.template)
                .join("article.html"),
        )?;
        let parser = liquid::ParserBuilder::with_stdlib().build().unwrap();
        let index_template = parser.parse(&index_template)?;
        let article_template = parser.parse(&article_template)?;
        let footer = render_markdown(read_to_string(workspace.join("footer.md"))?);
        Ok(Self {
            article_template,
            index_template,
            footer,
            config,
        })
    }

    pub fn template_path(&self) -> Result<PathBuf> {
        let workspace = workspace()?;
        Ok(workspace.join("template").join(&self.config.template))
    }
}
