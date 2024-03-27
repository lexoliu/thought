use std::{env::current_dir, process::Command};

use dialoguer::Select;
use thought::{workspace::WorkspaceBuilder, Config, Result};
pub fn command(name: String) -> Result<()> {
    let select = Select::new()
        .with_prompt("Choose a template for your blog ✨")
        .items(&["Zenflow", "Not now"])
        .interact()
        .unwrap();

    let mut workspace = WorkspaceBuilder::init(current_dir()?.join(&name))?;
    match select {
        0 => {
            log::info!("Installing theme");
            let output = Command::new("git")
                .args(["clone", "https://github.com/lexoooooo/zenflow"])
                .current_dir(format!("./{name}/template"))
                .output()?;

            if !output.status.success() {
                log::error!("{}", String::from_utf8_lossy(&output.stderr));
            }

            workspace.set_config(Config::new("zenflow"))?;
        }
        1 => {
            panic!("What the hell is going on???")
        }
        _ => {}
    }

    Ok(())
}
