use crate::{config::Config, create_file, utils::Error};
use std::{
    fs::{create_dir, File},
    path::PathBuf,
    process::Command,
};

pub fn command(name: String) -> Result<(), Error> {
    let mut path = PathBuf::from("./");

    path.set_file_name(&name);
    if File::open(path.join("Thought.toml")).is_ok() {
        return Err(Error::WorkspaceAlreadyExists);
    }

    Command::new("git").args(["init", &name]).output()?;

    create_file(
        path.join("Thought.toml"),
        toml::to_string_pretty(&Config::default()).unwrap(),
    )?;
    create_file(path.join("footer.md"), "Powered by Thought")?;
    create_file(path.join(".gitignore"), "/build")?;
    create_dir(path.join("template"))?;
    create_dir(path.join("articles"))?;

    log::info!("Create blog `{name}` successfully");

    Ok(())
}
