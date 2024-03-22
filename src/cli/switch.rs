use std::process::Command;

use thought::{Result, Workspace};

pub fn command<'a>(category: impl Iterator<Item = &'a str>) -> Result<()> {
    let workspace = Workspace::current()?;
    let mut path = workspace.path().to_owned();
    path.extend(category);
    Command::new("cd").arg(path);
    Ok(())
}
