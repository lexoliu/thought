use thought::{Category, Result, Workspace};

pub fn new(category: Vec<String>, name: String) -> Result<()> {
    let workspace = Workspace::current()?;
    Category::create(workspace, category, name)?;
    Ok(())
}
