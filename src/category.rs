use std::{fs::read_dir, ops::Deref, slice::Iter, vec::IntoIter};

use crate::{Error, Result, Workspace};

#[derive(Debug)]
pub struct Category {
    workspace: Workspace,
    path: Vec<String>,
}

impl Category {
    pub fn open(workspace: Workspace, path: Vec<String>) -> Self {
        Self { workspace, path }
    }
    pub fn list_categories(&self) -> Result<Vec<Self>> {
        let mut path = self.workspace.path().to_owned();
        path.extend(&self.path);
        let mut categories = Vec::new();
        for item in read_dir(path)? {
            let item = item?;
            let filetype = item.file_type()?;
            if filetype.is_dir() {
                let mut path = self.path.clone();
                path.push(
                    item.path()
                        .file_name()
                        .unwrap()
                        .to_str()
                        .ok_or(Error::IllegalCategoryName)?
                        .to_string(),
                );
                categories.push(Self {
                    workspace: self.workspace.clone(),
                    path,
                })
            }
        }
        Ok(categories)
    }
}

impl IntoIterator for Category {
    type IntoIter = IntoIter<Self::Item>;
    type Item = String;
    fn into_iter(self) -> Self::IntoIter {
        self.path.into_iter()
    }
}

impl<'a> IntoIterator for &'a Category {
    type IntoIter = Iter<'a, String>;
    type Item = &'a String;
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
