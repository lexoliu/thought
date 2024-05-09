mod category;
mod clean;
mod generate;
mod init;
mod new;
mod serve;
mod switch;
use std::path::PathBuf;

use clap::{Parser, Subcommand};
use itertools::Itertools;

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
        name: String,
        category: Option<String>,
    },
    Generate {
        output: Option<String>,
    },
    Switch {
        category: String,
    },
    Category {
        category: String,
        #[command(subcommand)]
        command: CategoryCommand,
    },
    Clean,
    Serve {
        port: Option<u16>,
    },
}

#[derive(Subcommand)]
enum CategoryCommand {
    New { name: String },
    Delete,
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
        CliCommand::New { name, category } => new::command(
            name,
            category
                .unwrap_or_default()
                .split('/')
                .map(String::from)
                .collect_vec(),
        ),
        CliCommand::Generate { output } => generate::command(output.map(PathBuf::from)),
        CliCommand::Switch { category } => switch::command(category.split('/')),
        CliCommand::Category { category, command } => match command {
            CategoryCommand::New { name } => {
                category::new(category.split('/').map(String::from).collect_vec(), name)
            }
            CategoryCommand::Delete => todo!(),
        },
        CliCommand::Clean => clean::command(),
        CliCommand::Serve { port } => serve::command(port),
    };

    if let Err(error) = result {
        log::error!("{error}");
    }
}
