use core::time::Duration;
use std::{
    env::current_dir,
    io::{self, Write},
    process::exit,
};

use crate::plugin::{PluginCommands, handle_plugin_command};
use clap::{Parser, Subcommand};
use color_eyre::{
    Section,
    config::HookBuilder,
    eyre::{self},
};
use indicatif::{ProgressBar, ProgressStyle};
use thought::{search::Searcher, serve, workspace::Workspace};
use tracing::{error, info, level_filters::LevelFilter};
use tracing_subscriber::{
    EnvFilter, filter::Directive, layer::SubscriberExt, util::SubscriberInitExt,
};
use translate::run_translate;

mod plugin;
mod translate;

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
        name: Option<String>,
    },

    #[command(subcommand)]
    Article(ArticleCommands),

    Generate,

    /// Search indexed articles with fuzzy, multilingual matching.
    Search {
        query: String,
    },

    /// Serve the workspace locally with lazy compilation.
    Serve {
        /// Host address to bind (default 127.0.0.1)
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        /// Port to listen on
        #[arg(short, long)]
        port: Option<u16>,
    },

    /// Plugin development helpers
    Plugin {
        #[command(subcommand)]
        command: PluginCommands,
    },

    /// Translate all articles into the given language code (uses OpenRouter).
    Translate {
        /// Target language code, e.g. zh-CN, ja, fr
        language: String,
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

#[tokio::main(flavor = "multi_thread")]
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
    let mut filter_layer = EnvFilter::builder()
        .with_default_directive(level.into())
        .from_env_lossy();
    if let Ok(directive) = "tantivy=warn".parse::<Directive>() {
        filter_layer = filter_layer.add_directive(directive);
    }

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
            let name = match name {
                Some(name) => name,
                None => prompt_blog_name()?,
            };
            Workspace::create(current_dir, name).await?;
            info!("Workspace created successfully");
            Ok(())
        }
        Commands::Plugin {
            command: plugin_cmd,
        } => {
            handle_plugin_command(plugin_cmd).await?;
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
                Commands::Serve { host, port } => {
                    let (port, allow_fallback) = match port {
                        Some(port) => (port, false),
                        None => (2006, true),
                    };
                    serve::serve(workspace.clone(), host, port, allow_fallback).await?;
                    Ok(())
                }
                Commands::Translate { language } => {
                    run_translate(workspace.clone(), language).await?;
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

fn prompt_blog_name() -> eyre::Result<String> {
    loop {
        print!("Blog name: ");
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let name = input.trim().to_string();

        if !name.is_empty() {
            return Ok(name);
        }

        println!("Blog name cannot be empty. Please enter a name.");
    }
}

async fn run_search(workspace: &Workspace, query: &str, emit_json: bool) -> eyre::Result<()> {
    let searcher = Searcher::open(workspace.clone())
        .await
        .note("Failed to open search index")?;
    long_task(
        "Indexing articles for search...",
        searcher.ensure_index(None),
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
