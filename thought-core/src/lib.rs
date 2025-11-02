pub mod article;
pub mod category;

#[non_exhaustive]
pub enum ParseError {
    IllegalAscii,
    MissingArticleName,
}
