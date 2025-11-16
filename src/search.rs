use std::path::Path;

use color_eyre::eyre::{self, eyre};
use futures::TryStreamExt;
use serde::Serialize;
use serde_json::json;
use tantivy::{
    Index, IndexWriter, Term,
    collector::TopDocs,
    doc,
    query::{BooleanQuery, FuzzyTermQuery, Occur, Query, QueryParser},
    schema::{
        Field, IndexRecordOption, OwnedValue, STORED, Schema, TantivyDocument, TextFieldIndexing,
        TextOptions,
    },
    tokenizer::{LowerCaser, RemoveLongFilter, SimpleTokenizer, TextAnalyzer},
};
use tokio::fs;
use unicode_segmentation::UnicodeSegmentation;
use wat::parse_str;

use crate::{article::Article, utils::write, workspace::Workspace};

const TOKENIZER: &str = "thought_tokenizer";

#[derive(Debug, Clone, Serialize)]
pub struct SearchHit {
    pub title: String,
    pub description: String,
    pub permalink: String,
}

impl From<Article> for SearchHit {
    fn from(article: Article) -> Self {
        let permalink = format!("{}.html", article.segments().join("/"));
        Self {
            title: article.title().to_string(),
            description: article.description().to_string(),
            permalink,
        }
    }
}

/// Indexer and search runner for workspace articles.
pub struct Searcher {
    workspace: Workspace,
    index: Index,
    title_field: Field,
    content_field: Field,
    path_field: Field,
}

impl Searcher {
    /// Open (or create) the search index located in `.thought/search_db`.
    pub async fn open(workspace: Workspace) -> eyre::Result<Self> {
        let cache_dir = workspace.cache_dir().join("search_db");
        if !fs::try_exists(&cache_dir).await? {
            fs::create_dir_all(&cache_dir).await?;
        }

        let schema = Self::build_schema();
        let meta_path = cache_dir.join("meta.json");
        let index = if fs::try_exists(&meta_path).await? {
            Index::open_in_dir(&cache_dir)?
        } else {
            Index::create_in_dir(&cache_dir, schema.clone())?
        };

        let analyzer = TextAnalyzer::builder(SimpleTokenizer::default())
            .filter(RemoveLongFilter::limit(40))
            .filter(LowerCaser)
            .build();
        index.tokenizers().register(TOKENIZER, analyzer);

        let schema = index.schema();
        let title_field = schema.get_field("title").map_err(|err| eyre!(err))?;
        let content_field = schema.get_field("content").map_err(|err| eyre!(err))?;
        let path_field = schema.get_field("path").map_err(|err| eyre!(err))?;

        Ok(Self {
            workspace,
            index,
            title_field,
            content_field,
            path_field,
        })
    }

    fn build_schema() -> Schema {
        let mut builder = Schema::builder();
        let text_field_indexing = TextFieldIndexing::default()
            .set_tokenizer(TOKENIZER)
            .set_index_option(IndexRecordOption::WithFreqsAndPositions);
        let stored_text = TextOptions::default()
            .set_indexing_options(text_field_indexing.clone())
            .set_stored();

        builder.add_text_field("title", stored_text.clone());
        builder.add_text_field(
            "content",
            TextOptions::default().set_indexing_options(text_field_indexing),
        );
        builder.add_text_field("path", STORED);
        builder.build()
    }

    /// Rebuild the search index from scratch.
    pub async fn index(&self) -> eyre::Result<()> {
        let mut writer: IndexWriter = self.index.writer(50_000_000)?;
        writer.delete_all_documents()?;

        let stream = self.workspace.articles();
        futures::pin_mut!(stream);
        while let Some(article) = stream.as_mut().try_next().await? {
            let doc = doc!(
                self.title_field => article.title().to_string(),
                self.content_field => article.content().to_string(),
                self.path_field => article.dir().to_string_lossy().to_string(),
            );
            writer.add_document(doc)?;
        }

        writer.commit()?;
        Ok(())
    }

    /// Search for a query string, returning fuzzy matches.
    pub async fn search(&self, query: &str, limit: usize) -> eyre::Result<Vec<SearchHit>> {
        if query.trim().is_empty() {
            return Ok(Vec::new());
        }

        let reader = self.index.reader()?;
        let searcher = reader.searcher();

        let query_parser =
            QueryParser::for_index(&self.index, vec![self.title_field, self.content_field]);
        let parsed = query_parser.parse_query(query)?;

        let mut subqueries: Vec<(Occur, Box<dyn Query>)> = vec![(Occur::Should, parsed)];
        for token in Self::tokenize(query) {
            let content_term = Term::from_field_text(self.content_field, &token);
            let title_term = Term::from_field_text(self.title_field, &token);
            subqueries.push((
                Occur::Should,
                Box::new(FuzzyTermQuery::new(content_term, 2, true)),
            ));
            subqueries.push((
                Occur::Should,
                Box::new(FuzzyTermQuery::new(title_term, 2, true)),
            ));
        }

        let query = BooleanQuery::new(subqueries);
        let top_docs = searcher.search(&query, &TopDocs::with_limit(limit))?;

        let mut hits = Vec::new();
        for (_score, address) in top_docs {
            let doc: TantivyDocument = searcher.doc(address)?;
            if let Some(path_value) = doc.get_first(self.path_field) {
                let owned: OwnedValue = path_value.into();
                if let OwnedValue::Str(path) = owned {
                    let article = self.workspace.read_article(Path::new(&path)).await?;
                    hits.push(article.into());
                }
            }
        }
        Ok(hits)
    }

    fn tokenize(input: &str) -> Vec<String> {
        input
            .unicode_words()
            .filter(|token| !token.trim().is_empty())
            .map(|token| token.to_string())
            .collect()
    }

    /// Emit a WASM-friendly JSON payload containing article metadata for client-side search fallback.
    pub async fn build_wasm(&self, output: impl AsRef<Path>) -> eyre::Result<()> {
        let payload = self.export_records().await?;
        let wasm = Self::encode_payload_as_wasm(&payload)?;

        if let Some(parent) = output.as_ref().parent() {
            fs::create_dir_all(parent).await?;
        }

        write(output, wasm).await?;
        Ok(())
    }

    async fn export_records(&self) -> eyre::Result<Vec<u8>> {
        let mut records = Vec::new();
        let stream = self.workspace.articles();
        futures::pin_mut!(stream);
        while let Some(article) = stream.as_mut().try_next().await? {
            records.push(json!({
                "title": article.title(),
                "slug": article.slug(),
                "category": article.category().segments(),
                "description": article.description(),
                "permalink": format!("{}.html", article.segments().join("/")),
            }));
        }

        Ok(serde_json::to_vec(&records)?)
    }

    fn encode_payload_as_wasm(payload: &[u8]) -> eyre::Result<Vec<u8>> {
        let pages = ((payload.len() as u32 + 0xFFFF) / 0x10000).max(1);
        let encoded_data = Self::encode_wat_bytes(payload);
        let module = format!(
            r#"(module
                (memory (export "memory") {pages})
                (func (export "thought_search_data_len") (result i32) (i32.const {len}))
                (func (export "thought_search_data_ptr") (result i32) (i32.const 0))
                (data (i32.const 0) "{data}")
            )"#,
            pages = pages,
            len = payload.len(),
            data = encoded_data
        );
        let wasm = parse_str(&module).map_err(|err| eyre!(err))?;
        Ok(wasm)
    }

    fn encode_wat_bytes(bytes: &[u8]) -> String {
        bytes
            .iter()
            .map(|byte| format!("\\{:02x}", byte))
            .collect::<String>()
    }
}
