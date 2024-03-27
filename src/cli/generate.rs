use std::path::PathBuf;

use thought::{Result, Workspace};
pub fn command(output: Option<PathBuf>) -> Result<()> {
    let workspace = Workspace::current()?;
    if let Some(output) = output {
        workspace.generate_to(output)?;
    } else {
        workspace.generate()?;
    }
    Ok(())
}
