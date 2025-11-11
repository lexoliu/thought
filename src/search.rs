use std::{io, path::Path};

use crate::{article::Article, utils::write, workspace::Workspace};
use serde_json::json;
use tantivy::{
    Index, IndexWriter,
    collector::TopDocs,
    query::QueryParser,
    schema::{Field, STORED, Schema, TEXT},
};
use tokio_stream::StreamExt;

/// A searcher that can index and search articles in a workspace.
pub struct Searcher {
    workspace: Workspace,
    index: Index,
    title_field: Field,
    content_field: Field,
    path_field: Field,
}

impl Searcher {
    /// Open a searcher for the given workspace, using the specified cache file path.
    pub async fn open(workspace: Workspace) -> io::Result<Self> {
        let cache = workspace.cache_dir().join("search_db");

        let mut schema_builder = Schema::builder();
        let title_field = schema_builder.add_text_field("title", TEXT | STORED);
        let content_field = schema_builder.add_text_field("content", TEXT);
        let path_field = schema_builder.add_text_field("path", STORED);
        let schema = schema_builder.build();

        let index = if cache.exists() {
            Index::open_in_dir(&cache).map_err(|e| io::Error::new(io::ErrorKind::Other, e))?
        } else {
            tokio::fs::create_dir_all(&cache).await?;
            Index::create_in_dir(&cache, schema.clone())
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?
        };

        Ok(Self {
            workspace,
            index,
            title_field,
            content_field,
            path_field,
        })
    }

    pub async fn index(&self) -> io::Result<()> {
        let mut writer: IndexWriter = self
            .index
            .writer(50_000_000)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

        writer
            .delete_all_documents()
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

        while let Some(article) = self.workspace.articles().try_next().await? {
            let mut doc = tantivy::Document::default();
            doc.add_text(self.title_field, &article.title);
            doc.add_text(self.content_field, &article.content);
            doc.add_text(self.path_field, article.path.to_string_lossy());
            writer
                .add_document(doc)
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        }

        writer
            .commit()
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

        Ok(())
    }

    pub async fn search(&self, query: &str) -> io::Result<Vec<Article>> {
        let reader = self
            .index
            .reader()
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        let searcher = reader.searcher();

        let query_parser =
            QueryParser::for_index(&self.index, vec![self.title_field, self.content_field]);
        let query = query_parser
            .parse_query(query)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

        let top_docs = searcher
            .search(&query, &TopDocs::with_limit(10))
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

        let mut results = Vec::new();
        for (_score, doc_address) in top_docs {
            let retrieved_doc = searcher
                .doc(doc_address)
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

            if let Some(path_value) = retrieved_doc.get_first(self.path_field) {
                if let Some(path_str) = path_value.as_text() {
                    let article = self.workspace.read_article(Path::new(path_str)).await?;
                    results.push(article);
                }
            }
        }

        Ok(results)
    }

    pub async fn build_wasm(&self, output: impl AsRef<Path>) -> io::Result<()> {
        let mut records = Vec::new();
        let mut stream = Box::pin(self.workspace.articles());
        while let Some(result) = stream.next().await {
            let article = result.map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;
            records.push(json!({
                "title": article.title(),
                "slug": article.slug(),
                "category": article.category().segments(),
                "description": article.description(),
            }));
        }

        if let Some(parent) = output.as_ref().parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let payload = serde_json::to_vec(&records)
            .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;
        write(output, &payload).await
    }
}
