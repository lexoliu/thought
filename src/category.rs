use itertools::Itertools;
use std::{
    fs::read_dir,
    ops::Deref,
    path::{Path, PathBuf},
    slice::Iter,
    vec::IntoIter,
};

use crate::article::ArticlePreview;
use crate::{Error, Result, Workspace};

#[derive(Debug, Clone)]
pub struct Category {
    workspace: Workspace,
    path: Vec<String>,
}

impl Category {
    pub const fn open(workspace: Workspace, path: Vec<String>) -> Self {
        Self { workspace, path }
    }

    pub fn from_dir(workspace: Workspace, path: impl AsRef<Path>) -> Result<Self> {
        let path = path
            .as_ref()
            .canonicalize()?
            .components()
            .map(|component| String::from_utf8(component.as_os_str().as_encoded_bytes().to_vec()))
            .try_collect()?;
        Ok(Self { workspace, path })
    }

    pub fn workspace(&self) -> &Workspace {
        &self.workspace
    }

    pub fn path(&self) -> PathBuf {
        let mut path = self.workspace.path().to_owned();
        path.extend(&self.path);
        path
    }

    pub fn categories(&self) -> Result<impl Iterator<Item = Result<Category>> + '_> {
        Ok(read_dir(self.path())?.map(|entry| {
            entry
                .map_err(Error::from)
                .and_then(|entry| Category::from_dir(self.workspace().clone(), entry.path()))
        }))
    }

    pub fn articles(&self) -> Result<impl Iterator<Item = Result<ArticlePreview>> + '_> {
        Ok(read_dir(self.path())?.map(|entry| {
            entry
                .map_err(Error::from)
                .and_then(|entry| ArticlePreview::from_dir(self.workspace().clone(), entry.path()))
        }))
    }
}

impl IntoIterator for Category {
    type Item = String;
    type IntoIter = IntoIter<Self::Item>;
    fn into_iter(self) -> Self::IntoIter {
        self.path.into_iter()
    }
}

impl<'a> IntoIterator for &'a Category {
    type Item = &'a String;
    type IntoIter = Iter<'a, String>;
    fn into_iter(self) -> Self::IntoIter {
        self.path.iter()
    }
}

impl Deref for Category {
    type Target = [String];
    fn deref(&self) -> &Self::Target {
        self.path.deref()
    }
}

impl AsRef<[String]> for Category {
    fn as_ref(&self) -> &[String] {
        self.deref()
    }
}
