use thought::{Result, Workspace};

pub fn command(name: String, category: Vec<String>) -> Result<()> {
    let workspace = Workspace::current()?;
    workspace.create_article(category, name)?;
    Ok(())
}
