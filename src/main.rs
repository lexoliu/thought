pub mod article;
mod build;
pub mod config;
mod init;
mod new;
mod switch;
pub mod utils;

use clap::{Parser, Subcommand};
use std::{fs::File, io::Write, path::Path};

#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    command: CliCommand,
}

#[derive(Subcommand)]
enum CliCommand {
    Init {
        name: String,
    },
    New {
        title: String,
        category: Option<String>,
    },
    Build {
        output: Option<String>,
        config: Option<String>,
    },
    Switch {
        category: String,
    },
}

fn main() {
    env_logger::Builder::new()
        .default_format()
        .filter_level(log::LevelFilter::Info)
        .format_timestamp(None)
        .format_module_path(false)
        .format_target(false)
        .init();
    let cli = Cli::parse();
    let result = match cli.command {
        CliCommand::Init { name } => init::command(name),
        CliCommand::New { title, category } => new::command(
            &title,
            category.as_ref().map(|category| category.split('/')),
        ),
        CliCommand::Build { .. } => build::command(None),
        CliCommand::Switch { category } => switch::command(category.split('/')),
    };

    if let Err(error) = result {
        log::error!("{error}")
    }
}

fn create_file(path: impl AsRef<Path>, content: impl AsRef<[u8]>) -> std::io::Result<()> {
    let mut file = File::create(path)?;
    file.write_all(content.as_ref())?;
    Ok(())
}
