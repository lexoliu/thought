use std::{env::current_dir, process::exit};

use clap::{Parser, Subcommand};
use color_eyre::{
    Section,
    config::HookBuilder,
    eyre::{self, Context},
};
use thought::workspace::Workspace;
use tracing::{error, level_filters::LevelFilter};
use tracing_subscriber::{EnvFilter, FmtSubscriber, layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Parser)]
#[command(about = "Build your thoughts", long_about = None)]
#[command(version, author)]
struct Cli {
    /// Increase output verbosity (-v, -vv)
    #[arg(short, long, action = clap::ArgAction::Count, global = true)]
    verbose: u8,

    /// Emit machine-readable JSON output (shorthand for --format json)
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Commands,
}

impl Cli {
    pub fn require_workspace(&self) -> bool {
        !matches!(&self.command, Commands::Create { .. })
    }
}

#[derive(Subcommand)]
enum Commands {
    // create a new workspace
    Create {
        name: String,
    },

    #[command(subcommand)]
    Article(ArticleCommands),

    Generate,
}

#[derive(Subcommand)]
enum ArticleCommands {
    // create a new article
    Create {
        title: String,
        category: Option<String>,
    },
}

#[tokio::main]
async fn main() {
    HookBuilder::default()
        .display_env_section(false)
        .issue_url("https://github.com/lexoliu/thought/issues/new")
        .panic_section("It looks like Thought encountered a bug")
        .install()
        .expect("Failed to install color-eyre hook");

    let cli = Cli::parse();

    let level: LevelFilter = match cli.verbose {
        0 => LevelFilter::WARN,
        1 => LevelFilter::INFO,
        2 => LevelFilter::DEBUG,
        _ => LevelFilter::TRACE,
    };

    let fmt_layer = tracing_subscriber::fmt::layer()
        .without_time()
        .with_target(false);
    let filter_layer = EnvFilter::builder()
        .with_default_directive(level.into())
        .from_env_lossy();

    tracing_subscriber::registry()
        .with(filter_layer)
        .with(fmt_layer)
        .with(tracing_error::ErrorLayer::default())
        .init();

    if let Err(err) = entry(cli).await {
        error!("{:#}", err);
        exit(1);
    }
}

async fn entry(cli: Cli) -> eyre::Result<()> {
    if cli.require_workspace() {
        let current_dir = current_dir()?;

        let workspace = Workspace::open(&current_dir)
            .await
            .note("Can't open workspace")?;
        workspace.generate(current_dir.join("build")).await?;
    }

    panic!("Not implemented yet");

    Ok(())
}
