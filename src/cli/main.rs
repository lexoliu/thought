mod build;
mod init;
mod new;
mod switch;

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
            title,
            category
                .unwrap_or_default()
                .split('/')
                .map(String::from)
                .collect_vec(),
        ),
        CliCommand::Build { .. } => build::command(None),
        CliCommand::Switch { category } => switch::command(category.split('/')),
    };

    if let Err(error) = result {
        log::error!("{error}")
    }
}
