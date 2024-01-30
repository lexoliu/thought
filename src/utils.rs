use pulldown_cmark::{html::push_html, Event};
use std::{
    fs::File,
    io::{BufReader, Read, Write},
    path::Path,
};

pub fn create_file(path: impl AsRef<Path>, content: impl AsRef<[u8]>) -> std::io::Result<()> {
    let mut file = File::create(path)?;
    file.write_all(content.as_ref())?;
    Ok(())
}

pub fn read_to_string(path: impl AsRef<Path>) -> std::io::Result<String> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut buf = String::new();
    reader.read_to_string(&mut buf)?;
    Ok(buf)
}

pub fn to_html<'a>(iter: impl IntoIterator<Item = Event<'a>>) -> String {
    let mut html = String::new();
    push_html(&mut html, iter.into_iter());
    html
}

pub fn render_markdown(markdown: impl AsRef<str>) -> String {
    let parser = pulldown_cmark::Parser::new(markdown.as_ref());
    to_html(parser)
}

pub fn not_found<T, E: From<std::io::Error>>(original: std::io::Result<T>, to: E) -> Result<T, E> {
    original.map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            to
        } else {
            error.into()
        }
    })
}
