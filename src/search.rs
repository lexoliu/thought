use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};

use color_eyre::eyre::{self, eyre};
use futures::TryStreamExt;
use rayon::prelude::*;
use redb::{Database, ReadableDatabase, TableDefinition};
use serde::Serialize;
use serde_json::json;
use tantivy::{
    Index, Term,
    collector::TopDocs,
    doc,
    query::{BooleanQuery, FuzzyTermQuery, Occur, Query, QueryParser},
    schema::{
        Field, IndexRecordOption, OwnedValue, STORED, Schema, TantivyDocument, TextFieldIndexing,
        TextOptions,
    },
    tokenizer::{LowerCaser, NgramTokenizer, RemoveLongFilter, TextAnalyzer},
};
use tokio::{fs, task::spawn_blocking};
use unicode_segmentation::UnicodeSegmentation;
use wasm_encoder::{
    CodeSection, ConstExpr, DataSection, ExportKind, ExportSection, Function, FunctionSection,
    Instruction, MemorySection, MemoryType, Module, TypeSection, ValType,
};

use crate::{article::Article, utils::write, workspace::Workspace};
use thought_plugin::helpers::{search_asset_dir, search_js_filename, search_wasm_filename};

pub(crate) const SEARCH_WRAPPER: &str = include_str!("../assets/thought-search.js");

const TOKENIZER: &str = "thought_tokenizer";
const SEARCH_META_TABLE: TableDefinition<&str, &str> = TableDefinition::new("search_meta");
const INDEX_WRITER_MEMORY: usize = 256 * 1024 * 1024;
const FIELD_TITLE: &str = "title";
const FIELD_CONTENT: &str = "content";
const FIELD_DESCRIPTION: &str = "description";
const FIELD_PERMALINK: &str = "permalink";
const FIELD_LOCALE: &str = "locale";
const FIELD_DEFAULT_LOCALE: &str = "default_locale";
const FIELD_SLUG: &str = "slug";
const FIELD_CATEGORY: &str = "category";

#[derive(Debug, Clone, Serialize)]
pub struct SearchHit {
    pub title: String,
    pub description: String,
    pub permalink: String,
    pub locale: String,
    pub default_locale: String,
    pub slug: String,
    pub category: Vec<String>,
}

impl From<Article> for SearchHit {
    fn from(article: Article) -> Self {
        let permalink = article.output_file();
        Self {
            title: article.title().to_string(),
            description: article.description().to_string(),
            permalink,
            locale: article.locale().to_string(),
            default_locale: article.default_locale().to_string(),
            slug: article.slug().to_string(),
            category: article.category().segments().clone(),
        }
    }
}

/// Indexer and search runner for workspace articles.
pub struct Searcher {
    workspace: Workspace,
    index: Index,
    meta_db: Arc<Database>,
    title_field: Field,
    content_field: Field,
    description_field: Field,
    permalink_field: Field,
    locale_field: Field,
    default_locale_field: Field,
    slug_field: Field,
    category_field: Field,
}

struct IndexedDoc {
    title: String,
    content: String,
    description: String,
    permalink: String,
    locale: String,
    default_locale: String,
    slug: String,
    category: String,
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
        let mut index = if fs::try_exists(&meta_path).await? {
            match Index::open_in_dir(&cache_dir) {
                Ok(index) => index,
                Err(_) => {
                    fs::remove_dir_all(&cache_dir).await?;
                    fs::create_dir_all(&cache_dir).await?;
                    Index::create_in_dir(&cache_dir, schema.clone())?
                }
            }
        } else {
            Index::create_in_dir(&cache_dir, schema.clone())?
        };
        if !Self::schema_compatible(&index.schema()) {
            drop(index);
            fs::remove_dir_all(&cache_dir).await?;
            fs::create_dir_all(&cache_dir).await?;
            index = Index::create_in_dir(&cache_dir, schema.clone())?;
        }

        let ngram = NgramTokenizer::new(1, 3, false).map_err(|err| eyre!(err))?;
        let analyzer = TextAnalyzer::builder(ngram)
            .filter(RemoveLongFilter::limit(40))
            .filter(LowerCaser);
        let analyzer = analyzer.build();
        index.tokenizers().register(TOKENIZER, analyzer);

        let schema = index.schema();
        let title_field = schema.get_field(FIELD_TITLE).map_err(|err| eyre!(err))?;
        let content_field = schema.get_field(FIELD_CONTENT).map_err(|err| eyre!(err))?;
        let description_field = schema
            .get_field(FIELD_DESCRIPTION)
            .map_err(|err| eyre!(err))?;
        let permalink_field = schema
            .get_field(FIELD_PERMALINK)
            .map_err(|err| eyre!(err))?;
        let locale_field = schema.get_field(FIELD_LOCALE).map_err(|err| eyre!(err))?;
        let default_locale_field = schema
            .get_field(FIELD_DEFAULT_LOCALE)
            .map_err(|err| eyre!(err))?;
        let slug_field = schema.get_field(FIELD_SLUG).map_err(|err| eyre!(err))?;
        let category_field = schema.get_field(FIELD_CATEGORY).map_err(|err| eyre!(err))?;

        let meta_db_path = workspace.cache_dir().join("search_index.redb");
        let meta_db = open_meta_database(meta_db_path).await?;
        ensure_meta_table(&meta_db).await?;

        Ok(Self {
            workspace,
            index,
            meta_db,
            title_field,
            content_field,
            description_field,
            permalink_field,
            locale_field,
            default_locale_field,
            slug_field,
            category_field,
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

        builder.add_text_field(FIELD_TITLE, stored_text.clone());
        builder.add_text_field(
            FIELD_CONTENT,
            TextOptions::default().set_indexing_options(text_field_indexing),
        );
        builder.add_text_field(FIELD_DESCRIPTION, STORED);
        builder.add_text_field(FIELD_PERMALINK, STORED);
        builder.add_text_field(FIELD_LOCALE, STORED);
        builder.add_text_field(FIELD_DEFAULT_LOCALE, STORED);
        builder.add_text_field(FIELD_SLUG, STORED);
        builder.add_text_field(FIELD_CATEGORY, STORED);
        builder.build()
    }

    fn schema_compatible(schema: &Schema) -> bool {
        [
            FIELD_TITLE,
            FIELD_CONTENT,
            FIELD_DESCRIPTION,
            FIELD_PERMALINK,
            FIELD_LOCALE,
            FIELD_DEFAULT_LOCALE,
            FIELD_SLUG,
            FIELD_CATEGORY,
        ]
        .iter()
        .all(|field| schema.get_field(field).is_ok())
    }

    /// Rebuild the search index from scratch.
    pub async fn index(&self) -> eyre::Result<()> {
        let writer = self.index.writer(INDEX_WRITER_MEMORY)?;
        writer.delete_all_documents()?;

        let mut docs = Vec::new();
        let stream = self.workspace.articles();
        futures::pin_mut!(stream);
        while let Some(article) = stream.as_mut().try_next().await? {
            docs.push(IndexedDoc {
                title: article.title().to_string(),
                content: article.content().to_string(),
                description: article.description().to_string(),
                permalink: article.output_file(),
                locale: article.locale().to_string(),
                default_locale: article.default_locale().to_string(),
                slug: article.slug().to_string(),
                category: Self::encode_category(article.category().segments())?,
            });
        }

        let writer = Arc::new(writer);
        let title_field = self.title_field;
        let content_field = self.content_field;
        let description_field = self.description_field;
        let permalink_field = self.permalink_field;
        let locale_field = self.locale_field;
        let default_locale_field = self.default_locale_field;
        let slug_field = self.slug_field;
        let category_field = self.category_field;

        docs.par_iter().for_each(|entry| {
            let document = doc!(
                title_field => entry.title.as_str(),
                content_field => entry.content.as_str(),
                description_field => entry.description.as_str(),
                permalink_field => entry.permalink.as_str(),
                locale_field => entry.locale.as_str(),
                default_locale_field => entry.default_locale.as_str(),
                slug_field => entry.slug.as_str(),
                category_field => entry.category.as_str(),
            );
            let _ = writer.add_document(document);
        });

        let mut writer =
            Arc::try_unwrap(writer).map_err(|_| eyre!("search writer still in use"))?;
        writer.commit()?;
        Ok(())
    }

    /// Rebuild the index only when the provided fingerprint differs from the cached value.
    pub async fn ensure_index(&self, fingerprint: Option<&str>) -> eyre::Result<bool> {
        if let Some(expected) = fingerprint {
            if let Some(current) = self.read_fingerprint().await? {
                if current == expected {
                    return Ok(false);
                }
            }
            self.index().await?;
            self.write_fingerprint(expected).await?;
            return Ok(true);
        }

        self.index().await?;
        Ok(true)
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
            hits.push(self.search_hit_from_doc(&doc)?);
        }
        Ok(prefer_default_locale(hits))
    }

    fn search_hit_from_doc(&self, doc: &TantivyDocument) -> eyre::Result<SearchHit> {
        let category = Self::stored_string(doc, self.category_field, FIELD_CATEGORY)?;
        Ok(SearchHit {
            title: Self::stored_string(doc, self.title_field, FIELD_TITLE)?,
            description: Self::stored_string(doc, self.description_field, FIELD_DESCRIPTION)?,
            permalink: Self::stored_string(doc, self.permalink_field, FIELD_PERMALINK)?,
            locale: Self::stored_string(doc, self.locale_field, FIELD_LOCALE)?,
            default_locale: Self::stored_string(
                doc,
                self.default_locale_field,
                FIELD_DEFAULT_LOCALE,
            )?,
            slug: Self::stored_string(doc, self.slug_field, FIELD_SLUG)?,
            category: Self::decode_category(&category)?,
        })
    }

    fn stored_string(
        doc: &TantivyDocument,
        field: Field,
        field_name: &str,
    ) -> eyre::Result<String> {
        let value = doc
            .get_first(field)
            .ok_or_else(|| eyre!("search index document missing `{field_name}`"))?;
        match OwnedValue::from(value) {
            OwnedValue::Str(text) => Ok(text),
            other => Err(eyre!(
                "search index document field `{field_name}` has invalid value: {other:?}"
            )),
        }
    }

    fn encode_category(segments: &[String]) -> eyre::Result<String> {
        if let Some(segment) = segments.iter().find(|segment| segment.contains('/')) {
            return Err(eyre!(
                "category segment `{segment}` contains reserved separator `/`"
            ));
        }
        Ok(segments.join("/"))
    }

    fn decode_category(encoded: &str) -> eyre::Result<Vec<String>> {
        if encoded.is_empty() {
            return Ok(Vec::new());
        }
        let segments = encoded
            .split('/')
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        if segments.iter().any(|segment| segment.is_empty()) {
            return Err(eyre!(
                "search index category `{encoded}` contains empty segment"
            ));
        }
        Ok(segments)
    }

    fn tokenize(input: &str) -> Vec<String> {
        let mut tokens: Vec<String> = input
            .unicode_words()
            .filter(|token| !token.trim().is_empty())
            .map(|token| token.to_string())
            .collect();

        if tokens.is_empty() || tokens.iter().all(|tok| tok.chars().count() <= 2) {
            tokens = input
                .graphemes(true)
                .filter(|g| !g.trim().is_empty())
                .map(|g| g.to_string())
                .collect();
        }

        tokens.sort();
        tokens.dedup();
        tokens
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
                "permalink": article.output_file(),
                "locale": article.locale(),
                "default_locale": article.default_locale(),
            }));
        }

        Ok(serde_json::to_vec(&records)?)
    }

    fn encode_payload_as_wasm(payload: &[u8]) -> eyre::Result<Vec<u8>> {
        let pages = (payload.len() as u64).div_ceil(0x10000).max(1);

        let mut module = Module::new();

        // Type section: () -> i32
        let mut types = TypeSection::new();
        types.ty().function([], [ValType::I32]);
        module.section(&types);

        // Function section
        let mut functions = FunctionSection::new();
        functions.function(0); // thought_search_data_len
        functions.function(0); // thought_search_data_ptr
        module.section(&functions);

        // Memory section
        let mut memories = MemorySection::new();
        memories.memory(MemoryType {
            minimum: pages,
            maximum: None,
            memory64: false,
            shared: false,
            page_size_log2: None,
        });
        module.section(&memories);

        // Export section
        let mut exports = ExportSection::new();
        exports.export("memory", ExportKind::Memory, 0);
        exports.export("thought_search_data_len", ExportKind::Func, 0);
        exports.export("thought_search_data_ptr", ExportKind::Func, 1);
        module.section(&exports);

        // Code section
        let mut code = CodeSection::new();

        let mut len_func = Function::new([]);
        #[expect(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
        let payload_len = payload.len() as i32;
        len_func.instruction(&Instruction::I32Const(payload_len));
        len_func.instruction(&Instruction::End);
        code.function(&len_func);

        let mut ptr_func = Function::new([]);
        ptr_func.instruction(&Instruction::I32Const(0));
        ptr_func.instruction(&Instruction::End);
        code.function(&ptr_func);

        module.section(&code);

        // Data section - direct binary, no encoding overhead
        let mut data = DataSection::new();
        let offset = ConstExpr::i32_const(0);
        data.active(0, &offset, payload.iter().copied());
        module.section(&data);

        Ok(module.finish())
    }
    async fn read_fingerprint(&self) -> eyre::Result<Option<String>> {
        let db = Arc::clone(&self.meta_db);
        spawn_blocking(move || -> eyre::Result<Option<String>> {
            let txn = db.begin_read()?;
            let table = txn.open_table(SEARCH_META_TABLE)?;
            let value = table.get("fingerprint")?;
            Ok(value.map(|guard| guard.value().to_string()))
        })
        .await?
    }

    async fn write_fingerprint(&self, fingerprint: &str) -> eyre::Result<()> {
        let db = Arc::clone(&self.meta_db);
        let fingerprint = fingerprint.to_string();
        spawn_blocking(move || -> eyre::Result<()> {
            let txn = db.begin_write()?;
            {
                let mut table = txn.open_table(SEARCH_META_TABLE)?;
                table.insert("fingerprint", fingerprint.as_str())?;
            }
            txn.commit()?;
            Ok(())
        })
        .await?
    }
}

pub async fn emit_search_bundle(
    workspace: &Workspace,
    output: &Path,
    fingerprint: Option<&str>,
) -> eyre::Result<()> {
    let searcher = Searcher::open(workspace.clone()).await?;
    searcher.ensure_index(fingerprint).await?;

    let asset_dir = output.join(search_asset_dir());
    fs::create_dir_all(&asset_dir).await?;

    let wasm_path = asset_dir.join(search_wasm_filename());
    searcher.build_wasm(&wasm_path).await?;

    let js_path = asset_dir.join(search_js_filename());
    write(js_path, SEARCH_WRAPPER.as_bytes()).await?;
    Ok(())
}

async fn open_meta_database(path: PathBuf) -> eyre::Result<Arc<Database>> {
    spawn_blocking(move || -> eyre::Result<Arc<Database>> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let db = if path.exists() {
            Database::open(path.as_path())?
        } else {
            Database::create(path.as_path())?
        };
        Ok(Arc::new(db))
    })
    .await?
}

async fn ensure_meta_table(db: &Arc<Database>) -> eyre::Result<()> {
    let db = Arc::clone(db);
    spawn_blocking(move || -> eyre::Result<()> {
        let txn = db.begin_write()?;
        txn.open_table(SEARCH_META_TABLE)?;
        txn.commit()?;
        Ok(())
    })
    .await?
}

fn prefer_default_locale(hits: Vec<SearchHit>) -> Vec<SearchHit> {
    let mut by_slug: HashMap<String, SearchHit> = HashMap::new();
    for hit in hits {
        let key = if hit.category.is_empty() {
            hit.slug.clone()
        } else {
            format!("{}/{}", hit.category.join("/"), hit.slug)
        };
        by_slug
            .entry(key)
            .and_modify(|existing| {
                let current_is_default = existing.locale == existing.default_locale;
                let candidate_is_default = hit.locale == hit.default_locale;
                if candidate_is_default && !current_is_default {
                    *existing = hit.clone();
                }
            })
            .or_insert(hit);
    }
    by_slug.into_values().collect()
}
