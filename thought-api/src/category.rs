use rkyv::Archive;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

#[derive(Debug, Clone, Archive)]
pub struct Category {
    path: Vec<String>,
    created: i64,
    name: String,
    description: String,
}
