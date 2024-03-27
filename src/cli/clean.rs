use thought::{Result, Workspace};

pub fn command() -> Result<()> {
    let workspace = Workspace::current()?;
    workspace.clean()
}
