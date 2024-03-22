use std::env::current_dir;

use thought::{Result, Workspace};

pub fn command(name: String) -> Result<()> {
    Workspace::init(current_dir()?.join(name))?;
    Ok(())
}
