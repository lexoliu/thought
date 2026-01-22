use std::{
    path::Path,
    sync::{Arc, LazyLock},
    time::Duration,
};

use aither::{
    LanguageModel,
    llm::{LLMRequest, Message},
    openai::OpenAI,
};
use color_eyre::eyre::{self, Context, eyre};
use dialoguer::{Select, theme::ColorfulTheme};
use futures::{StreamExt, TryStreamExt, pin_mut};
use indicatif::{ProgressBar, ProgressStyle};
use regex::Regex;
use tokio::time::sleep;
use tracing::{info, warn};

use thought::{article::Article, workspace::Workspace};

const LANGUAGE_EXAMPLES: [(&str, &str); 11] = [
    ("🇨🇳", "zh-CN"),
    ("🇹🇼", "zh-TW"),
    ("🇺🇸", "en-US"),
    ("🇯🇵", "ja-JP"),
    ("🇰🇷", "ko-KR"),
    ("🇪🇸", "es-ES"),
    ("🇫🇷", "fr-FR"),
    ("🇩🇪", "de-DE"),
    ("🇧🇷", "pt-BR"),
    ("🇷🇺", "ru-RU"),
    ("🇮🇳", "hi-IN"),
];

pub async fn run_translate(workspace: Workspace, language: Option<String>) -> eyre::Result<()> {
    let target = resolve_language_code(language)?;

    let config = workspace.manifest().translation_config();
    let models = config.effective_models();
    if models.is_empty() {
        return Err(eyre!("No translation models configured"));
    }
    let api_key = std::env::var("OPENROUTER_API_KEY")
        .wrap_err("Set OPENROUTER_API_KEY in your environment to enable translation")?;

    let jobs = collect_jobs(&workspace, &target).await?;
    if jobs.is_empty() {
        info!("All articles already have a {target} translation");
        return Ok(());
    }

    let pb = Arc::new(ProgressBar::new(jobs.len() as u64));
    pb.set_style(
        ProgressStyle::with_template(
            "{spinner:.green} {pos}/{len} translating ({msg}) {wide_bar:.cyan/blue}",
        )
        .unwrap()
        .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]),
    );
    pb.enable_steady_tick(Duration::from_millis(120));
    pb.set_message("starting");

    let concurrency = config.max_concurrency.max(1);
    let retries = config.max_retries;

    let mut errors = Vec::new();
    let mut stream = futures::stream::iter(jobs.into_iter().map(|article| {
        let target = target.clone();
        let pb = pb.clone();
        let models = models.clone();
        let api_key = api_key.clone();
        async move {
            pb.set_message(format!("{} → {target}", article.title()));
            match translate_article(&article, &target, &models, &api_key, retries).await {
                Ok(_) => {
                    pb.inc(1);
                    Ok(())
                }
                Err(err) => {
                    pb.inc(1);
                    pb.println(format!("✖ {}: {err:?}", article.title()));
                    Err(err)
                }
            }
        }
    }))
    .buffer_unordered(concurrency);

    while let Some(result) = stream.next().await {
        if let Err(err) = result {
            errors.push(err);
        }
    }

    if errors.is_empty() {
        pb.finish_with_message("translation complete");
        Ok(())
    } else {
        pb.finish_with_message("translation finished with errors");
        Err(eyre!(
            "{} translation(s) failed. See logs above.",
            errors.len()
        ))
    }
}

async fn collect_jobs(workspace: &Workspace, target: &str) -> eyre::Result<Vec<Article>> {
    let mut jobs = Vec::new();
    let mut stream = workspace.articles();
    while let Some(article) = stream.try_next().await? {
        if !article.is_default_locale() {
            continue;
        }
        if article.default_locale().eq_ignore_ascii_case(target.trim()) {
            continue;
        }
        if article
            .translations()
            .iter()
            .any(|t| t.locale().eq_ignore_ascii_case(target))
        {
            continue;
        }
        jobs.push(article);
    }
    Ok(jobs)
}

async fn translate_article(
    article: &Article,
    target: &str,
    models: &[String],
    api_key: &str,
    max_retries: usize,
) -> eyre::Result<()> {
    let prompt = build_prompt(article, target);
    let mut last_error = None;

    for model_name in models {
        let model = OpenAI::openrouter(api_key.to_string()).with_model(model_name.clone());
        for attempt in 0..=max_retries {
            match request_translation(model.clone(), &prompt).await {
                Ok(output) => {
                    let path = article.dir().join(format!("{target}.md"));
                    write_file(&path, &output).await?;
                    return Ok(());
                }
                Err(err) => {
                    last_error = Some(err);
                    if attempt < max_retries {
                        let backoff = Duration::from_secs(2u64.saturating_pow(attempt as u32 + 1));
                        warn!(
                            "Retrying translation for {} via {model_name} in {:?} (attempt {}/{})",
                            article.title(),
                            backoff,
                            attempt + 1,
                            max_retries
                        );
                        sleep(backoff).await;
                    }
                }
            }
        }
        warn!(
            "Model {model_name} failed for {}. Trying next model if available.",
            article.title()
        );
    }

    Err(last_error.unwrap_or_else(|| eyre!("translation failed")))
}

async fn request_translation(model: OpenAI, prompt: &str) -> eyre::Result<String> {
    let stream = model.respond(LLMRequest::new([
        Message::system(
            "You are a professional technical translator. Preserve Markdown structure, \
             keep code fences unchanged, and do not add commentary.",
        ),
        Message::user(prompt),
    ]));

    let mut output = String::new();
    pin_mut!(stream);
    while let Some(chunk) = stream.next().await {
        output.push_str(&chunk?);
    }
    Ok(output)
}

fn build_prompt(article: &Article, target: &str) -> String {
    format!(
        "Translate the following Markdown from language `{src}` into `{target}`. \
         Keep headings, links, and formatting intact. Only return translated Markdown, \
         no explanations.\n\n{body}",
        src = article.default_locale(),
        target = target,
        body = article.content()
    )
}

async fn write_file(path: &Path, contents: &str) -> eyre::Result<()> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(path, contents).await?;
    Ok(())
}

fn resolve_language_code(language: Option<String>) -> eyre::Result<String> {
    match language {
        Some(raw) if !raw.trim().is_empty() => parse_language_code(Some(raw)),
        _ => {
            let selection = prompt_language_selection()?;
            parse_language_code(Some(selection))
        }
    }
}

fn parse_language_code(language: Option<String>) -> eyre::Result<String> {
    let Some(input) = language else {
        return Err(missing_language_error());
    };
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(missing_language_error());
    }
    if !is_valid_language_code(trimmed) {
        return Err(invalid_language_error(trimmed));
    }
    Ok(trimmed.to_string())
}

fn missing_language_error() -> eyre::Report {
    eyre!(
        "Please provide a target language code (BCP-47, e.g. en-US).\nExamples: {}",
        format_language_examples()
    )
}

fn invalid_language_error(code: &str) -> eyre::Report {
    eyre!(
        "Invalid language code `{code}`. Use a BCP-47 style code with letters and optional region (e.g. en-US).\nExamples: {}",
        format_language_examples()
    )
}

fn format_language_examples() -> String {
    LANGUAGE_EXAMPLES
        .iter()
        .map(|(flag, code)| format!("{flag} {code}"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn prompt_language_selection() -> eyre::Result<String> {
    let options = LANGUAGE_EXAMPLES
        .iter()
        .map(|(flag, code)| format!("{flag} {code}"))
        .collect::<Vec<_>>();
    let choice = Select::with_theme(&ColorfulTheme::default())
        .with_prompt("Select target language")
        .default(0)
        .items(&options)
        .interact()
        .wrap_err("Failed to read language selection")?;
    Ok(LANGUAGE_EXAMPLES[choice].1.to_string())
}

fn is_valid_language_code(code: &str) -> bool {
    static LANGUAGE_CODE_RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^[A-Za-z]{2,8}(?:-[A-Za-z0-9]{2,8})*$")
            .expect("language code regex should compile")
    });
    LANGUAGE_CODE_RE.is_match(code)
}
