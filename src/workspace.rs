use crate::{article::Article, category::Category, utils::create_file, Config, Error, Result};
use std::{
    fs::create_dir,
    ops::Deref,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
};

#[derive(Debug, Clone)]
pub struct Workspace {
    inner: Arc<WorkspaceBuilder>,
}

impl Workspace {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Ok(WorkspaceBuilder::open(path)?.build())
    }

    pub fn init(path: impl AsRef<Path>) -> Result<Self> {
        Ok(WorkspaceBuilder::init(path)?.build())
    }

    pub fn create_article(&self, name: String, category: Category) -> Result<Article> {
        Article::create(self, name, category)
    }
}

impl Deref for Workspace {
    type Target = WorkspaceBuilder;
    fn deref(&self) -> &Self::Target {
        self.inner.deref()
    }
}

#[derive(Debug)]
pub struct WorkspaceBuilder {
    path: PathBuf,
    config: Config,
}

impl WorkspaceBuilder {
    pub fn init(dir: impl AsRef<Path>) -> Result<Self> {
        let dir = dir.as_ref();
        if dir.join("Thought.toml").exists() {
            return Err(Error::WorkspaceAlreadyExists);
        }
        // TODO: handle the error between git!
        Command::new("git").arg("init").output()?;

        let config = Config::default();

        create_file(dir.join("Thought.toml"), config.export())?;
        create_file(dir.join("footer.md"), "Powered by Thought")?;
        create_file(dir.join(".gitignore"), "/build")?;
        create_dir(dir.join("template"))?;
        create_dir(dir.join("articles"))?;
        Ok(Self {
            path: dir.to_owned(),
            config,
        })
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if !path.join("Thought.toml").exists() {
            return Err(Error::WorkspaceNotFound);
        }
        Ok(Self {
            path: path.to_owned(),
            config: Config::from_file(path.join("Thought.toml"))?,
        })
    }

    pub fn set_config(&mut self, config: Config) {
        self.config = config;
    }

    pub const fn config(&self) -> &Config {
        &self.config
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn template_path(&self) -> PathBuf {
        self.path.join("template").join(self.config.template())
    }

    pub fn build(self) -> Workspace {
        Workspace {
            inner: Arc::new(self),
        }
    }
}
