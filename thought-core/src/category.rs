use std::str::FromStr;

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::ParseError;

pub struct Category(Vec<String>);

impl FromStr for Category {
    type Err = ParseError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut vec: Vec<String> = Vec::new();
        for component in s.split('/') {
            if !component.is_ascii() {
                return Err(ParseError::IllegalAscii);
            }
            vec.push(component.to_string());
        }
        Ok(Self(vec))
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CategoryMetadata {
    #[serde(with = "time::serde::rfc3339")]
    created: OffsetDateTime,
    name: String,
    #[serde(default)]
    description: String,
}
