use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::PathBuf,
};

use sled::Tree;

use crate::article::Article;
use thought_core::article::ArticlePath;

pub struct Cache(Tree);

impl Cache {
    pub fn new() -> Self {
        todo!()
    }

    pub fn get(path: ArticlePath) -> Article {}
}
