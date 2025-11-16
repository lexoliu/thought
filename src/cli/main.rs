use core::time::Duration;
use std::{env::current_dir, process::exit};

use clap::{Parser, Subcommand};
use color_eyre::{
    Section,
    config::HookBuilder,
    eyre::{self},
};
use indicatif::{ProgressBar, ProgressStyle};
use thought::{search::Searcher, workspace::Workspace};
use tracing::{error, info, level_filters::LevelFilter};
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

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

#[derive(Subcommand)]
enum Commands {
    // create a new workspace
    Create {
        name: String,
    },

    #[command(subcommand)]
    Article(ArticleCommands),

    Generate,

    /// Search indexed articles with fuzzy, multilingual matching.
    Search {
        query: String,
    },
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
        .display_env_section(true)
        .issue_url("https://github.com/lexoliu/thought/issues/new")
        .panic_section("It looks like Thought encountered a bug")
        .install()
        .expect("Failed to install color-eyre hook");

    let cli = Cli::parse();

    let level = match cli.verbose {
        0 => LevelFilter::INFO,
        1 => LevelFilter::DEBUG,
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
    let current_dir = current_dir()?;
    let command = cli.command;

    match command {
        Commands::Create { name } => {
            Workspace::create(current_dir, name).await?;
            info!("Workspace created successfully");
            Ok(())
        }
        command => {
            let workspace = Workspace::open(&current_dir)
                .await
                .note("Can't open workspace")?;
            match command {
                Commands::Article(article_cmd) => match article_cmd {
                    ArticleCommands::Create { title, category: _ } => {
                        workspace
                            .create_article(title, None)
                            .await
                            .note("Failed to create article")?;
                        info!("Article created successfully");
                        Ok(())
                    }
                },
                Commands::Generate => {
                    long_task(
                        "Generating site...",
                        workspace.generate(workspace.build_dir()),
                        "Site generated successfully",
                    )
                    .await?;
                    Ok(())
                }
                Commands::Search { query } => {
                    run_search(&workspace, &query, cli.json).await?;
                    Ok(())
                }
                _ => unreachable!(),
            }
        }
    }
}

pub async fn long_task<T, E>(
    loading_msg: &'static str,
    f: impl Future<Output = Result<T, E>>,
    complete_msg: &'static str,
) -> Result<T, E> {
    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::with_template("{spinner:.green} {msg}")
            .unwrap()
            .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]),
    );
    pb.enable_steady_tick(Duration::from_millis(120));
    pb.set_message(loading_msg);

    let result = f.await?;

    pb.finish_with_message(complete_msg);
    Ok(result)
}

async fn run_search(workspace: &Workspace, query: &str, emit_json: bool) -> eyre::Result<()> {
    let searcher = Searcher::open(workspace.clone())
        .await
        .note("Failed to open search index")?;
    long_task(
        "Indexing articles for search...",
        searcher.index(),
        "Search index ready",
    )
    .await
    .note("Failed to build search index")?;

    let hits = searcher
        .search(query, 20)
        .await
        .note("Failed to search articles")?;

    if emit_json {
        println!("{}", serde_json::to_string_pretty(&hits)?);
        return Ok(());
    }

    if hits.is_empty() {
        println!("No results for \"{query}\"");
        return Ok(());
    }

    println!("Found {} result(s):", hits.len());
    for hit in hits {
        println!("• {} -> {}", hit.title, hit.permalink);
        if !hit.description.is_empty() {
            println!("  {}", hit.description);
        }
    }
    Ok(())
}
