use itertools::Itertools;
use serde::Serialize;
use std::{
    fs::{create_dir, read_dir},
    ops::Deref,
    path::{Path, PathBuf},
};

use crate::{article::ArticlePreview, metadata::CategoryMetadata};
use crate::{Error, Result, Workspace};

#[derive(Debug, Clone, Serialize)]
pub struct Category {
    #[serde(skip_serializing)]
    workspace: Workspace,
    path: Vec<String>,
    metadata: CategoryMetadata,
}

impl Category {
    pub fn open(workspace: Workspace, category_path: Vec<String>) -> Result<Self> {
        let path = workspace.category_path(&category_path);

        let metadata = CategoryMetadata::open(path.join(".category.toml"))?;
        Ok(Self {
            workspace,
            path: category_path,
            metadata,
        })
    }

    pub fn at(&self, category: impl ToComponents) -> Result<Self> {
        let category = category.to_components();
        let mut new = self.path().to_vec();
        new.extend(category);
        Self::open(self.workspace().clone(), new)
    }
    pub fn create(workspace: Workspace, category_path: Vec<String>, name: String) -> Result<Self> {
        create_dir(workspace.category_path(&category_path))?;
        let metadata = CategoryMetadata::create(
            workspace
                .category_path(&category_path)
                .join(".category.toml"),
            name,
        )?;
        Ok(Self {
            workspace,
            path: category_path,
            metadata,
        })
    }

    pub fn from_dir(workspace: Workspace, path: impl AsRef<Path>) -> Result<Self> {
        let category = path
            .as_ref()
            .canonicalize()?
            .strip_prefix(workspace.path().join("articles"))
            .map_err(|_| Error::WorkspaceNotFound)?
            .components()
            .map(|component| String::from_utf8(component.as_os_str().as_encoded_bytes().to_vec()))
            .try_collect()?;
        Self::open(workspace, category)
    }

    pub fn workspace(&self) -> &Workspace {
        &self.workspace
    }

    pub fn dir(&self) -> PathBuf {
        self.workspace().category_path(self.path())
    }

    pub fn path(&self) -> &[String] {
        &self.path
    }

    pub fn categories(&self) -> Result<impl Iterator<Item = Result<Category>>> {
        let workspace = self.workspace().clone();
        Ok(read_dir(self.dir())?
            .filter_ok(|entry| entry.path().join(".category.toml").exists())
            .map(move |entry| {
                entry
                    .map_err(Error::from)
                    .and_then(|entry| Category::from_dir(workspace.clone(), entry.path()))
            }))
    }

    pub fn articles(&self) -> Result<impl Iterator<Item = Result<ArticlePreview>>> {
        let workspace = self.workspace().clone();
        Ok(read_dir(self.dir())?
            .filter_ok(|entry| entry.path().join(".article.toml").exists())
            .map(move |entry| {
                entry
                    .map_err(Error::from)
                    .and_then(|entry| ArticlePreview::from_dir(workspace.clone(), entry.path()))
            }))
    }
    // bug here!
    pub fn all_categories(&self) -> Result<Box<dyn Iterator<Item = Result<Category>>>> {
        Ok(Box::new(
            self.categories()?
                .flat_map(|category| category.map(|c| c.all_categories()))
                .flatten()
                .flatten()
                .chain(self.categories()?),
        ))
    }

    pub fn all_articles(&self) -> Result<impl Iterator<Item = Result<ArticlePreview>>> {
        Ok(self
            .all_categories()?
            .flat_map(|category| category.map(|c| c.articles()))
            .flatten()
            .flatten()
            .chain(self.articles()?))
    }
}

pub trait ToComponents {
    fn to_components(self) -> impl Iterator<Item = String>;
}

impl ToComponents for &str {
    fn to_components(self) -> impl Iterator<Item = String> {
        self.split('/').map(String::from)
    }
}

impl ToComponents for String {
    fn to_components(self) -> impl Iterator<Item = String> {
        self.deref().to_components().collect_vec().into_iter()
    }
}

impl ToComponents for Vec<&str> {
    fn to_components(self) -> impl Iterator<Item = String> {
        self.iter()
            .map(ToString::to_string)
            .collect_vec()
            .into_iter()
    }
}

impl ToComponents for Vec<String> {
    fn to_components(self) -> impl Iterator<Item = String> {
        self.into_iter()
    }
}
