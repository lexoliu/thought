use std::process::Command;

use crate::utils::{workspace, Result};

pub fn command<'a>(category: impl Iterator<Item = &'a str>) -> Result<()> {
    let mut path = workspace()?.to_owned();
    path.extend(category);
    Command::new("cd").arg(path);
    Ok(())
}
