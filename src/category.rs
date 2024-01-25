use std::{convert::Infallible, ops::Deref, slice::Iter, str::FromStr, vec::IntoIter};

#[derive(Debug)]
pub struct Category {
    inner: Vec<String>,
}

impl FromStr for Category {
    type Err = Infallible;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self::new(
            s.split('/').map(String::from).collect::<Vec<_>>(),
        ))
    }
}

impl IntoIterator for Category {
    type IntoIter = IntoIter<Self::Item>;
    type Item = String;
    fn into_iter(self) -> Self::IntoIter {
        self.inner.into_iter()
    }
}

impl<'a> IntoIterator for &'a Category {
    type IntoIter = Iter<'a, String>;
    type Item = &'a String;
    fn into_iter(self) -> Self::IntoIter {
        self.inner.iter()
    }
}

impl Deref for Category {
    type Target = [String];
    fn deref(&self) -> &Self::Target {
        self.inner.deref()
    }
}

impl Category {
    pub fn new(category: impl Into<Vec<String>>) -> Self {
        Self {
            inner: category.into(),
        }
    }
}
