use crate::{
    article::{Article, ArticlePreview},
    category::{Category, ToComponents},
    generate::{generate, generate_footer, generate_index, template_engine},
    metadata::CategoryMetadata,
    utils::create_file,
    Config, Error, Result,
};
use std::{
    env::current_dir,
    fs::{create_dir, remove_dir_all},
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
};

#[derive(Debug, Clone)]
pub struct Workspace {
    inner: Arc<WorkspaceBuilder>,
}

impl Workspace {
    pub fn current() -> Result<Self> {
        Self::open(current_dir()?)
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Ok(WorkspaceBuilder::open(path)?.build())
    }

    pub fn init(path: impl AsRef<Path>) -> Result<Self> {
        Ok(WorkspaceBuilder::init(path)?.build())
    }

    pub fn create_article(&self, category: Vec<String>, name: String) -> Result<Article> {
        Article::create(Category::open(self.clone(), category)?, name)
    }

    pub fn article_path(&self, name: impl AsRef<str>, category: impl AsRef<[String]>) -> PathBuf {
        let mut path = self.path().join("articles");
        path.extend(category.as_ref());
        path.push(name.as_ref());
        path
    }

    pub fn category_path(&self, category: impl AsRef<[String]>) -> PathBuf {
        let mut path = self.path().join("articles");
        path.extend(category.as_ref());
        path
    }

    pub fn generate_to(&self, output: impl AsRef<Path>) -> Result<()> {
        generate(self, output.as_ref())
    }

    pub fn generate(&self) -> Result<()> {
        self.generate_to(self.generate_path())
    }

    pub fn generate_path(&self) -> PathBuf {
        self.path().join("build")
    }

    pub fn path(&self) -> &Path {
        self.inner.path()
    }

    pub fn config(&self) -> &Config {
        self.inner.config()
    }

    pub fn template_path(&self) -> PathBuf {
        self.inner.template_path()
    }

    pub fn root(&self) -> Result<Category> {
        Category::open(self.clone(), Vec::new())
    }

    pub fn all_articles(&self) -> Result<impl Iterator<Item = Result<ArticlePreview>>> {
        self.root()?.all_articles()
    }

    pub fn at(&self, category: impl ToComponents) -> Result<Category> {
        self.root()?.at(category)
    }

    pub fn render_index(&self) -> Result<String> {
        let engine = template_engine(self)?;
        let mut site_context = crate::generate::context::Site::new(self, "");

        let footer = generate_footer(&engine, site_context)?;
        site_context.set_footer(&footer);

        generate_index(&engine, self, site_context)
    }

    pub fn clean(&self) -> Result<()> {
        let path = self.path().join("build");
        if path.exists() {
            remove_dir_all(path)?;
        }

        Ok(())
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
        create_dir(dir)?;
        if dir.join("Thought.toml").exists() {
            return Err(Error::WorkspaceAlreadyExists);
        }

        // TODO: handle the error between git!
        Command::new("git").arg("init").output()?;

        let config = Config::new("[INSTALL ONE]");

        create_file(dir.join("Thought.toml"), config.export())?;
        create_file(dir.join("footer.md"), "Powered by Thought")?;
        create_file(dir.join(".gitignore"), "/build")?;
        create_dir(dir.join("template"))?;
        create_dir(dir.join("articles"))?;
        CategoryMetadata::create(dir.join("articles/.category.toml"), "root")?;
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

    pub const fn config(&self) -> &Config {
        &self.config
    }

    pub fn set_config(&mut self, config: Config) -> Result<()> {
        config.save(self.path().join("Thought.toml"))?;
        self.config = config;
        Ok(())
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
