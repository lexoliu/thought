use std::{
    env::current_dir,
    fs::{create_dir, File},
    io::{self, Write},
};

use time::OffsetDateTime;

use crate::{
    article::Metadata,
    utils::{workspace, Error, Result},
};

pub fn command<'a>(title: &str, category: Option<impl Iterator<Item = &'a str>>) -> Result<()> {
    let now = OffsetDateTime::now_utc();

    let mut path = workspace()?.join("articles");

    if let Some(category) = category {
        category.for_each(|item| {
            path.push(item);
        })
    } else {
        let current = current_dir()?;

        if current.starts_with(&path) {
            path = current
        }
    }

    let name: String = title
        .to_lowercase()
        .chars()
        .map(|s| if s == ' ' { '-' } else { s })
        .collect();

    create_dir(path.join(&name)).map_err(|error| {
        if error.kind() == io::ErrorKind::AlreadyExists {
            log::info!("Title duplicated, try another title or organise your post to into category with -c parameter");
            Error::PostAlreadyExists
        } else {
            error.into()
        }
    })?;

    let mut metadata = File::create(path.join(&name).join(".metadata.toml"))?;
    metadata.write_all(
        toml::to_string_pretty(&Metadata::new(now))
            .unwrap()
            .as_bytes(),
    )?;

    let mut content = File::create(path.join(&name).join("article.md"))?;
    content.write_all(format!("# {title}\n").as_bytes())?;

    Ok(())
}
