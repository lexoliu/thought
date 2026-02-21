use slug::slugify;
use std::{fmt, str::FromStr};

#[derive(Debug, thiserror::Error)]
#[error("generated article slug is empty")]
pub struct EmptySlug;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArticleSlug(String);

impl ArticleSlug {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    #[must_use]
    pub fn into_string(self) -> String {
        self.0
    }

    pub fn from_title(title: &str) -> Result<Self, EmptySlug> {
        let generated = slugify(title);
        ArticleSlug::from_str(&generated)
    }
}

impl AsRef<str> for ArticleSlug {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for ArticleSlug {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for ArticleSlug {
    type Err = EmptySlug;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return Err(EmptySlug);
        }

        Ok(Self(trimmed.to_string()))
    }
}
