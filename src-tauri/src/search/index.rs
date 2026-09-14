use crate::core::{SearchHit, SearchResponse};
use anyhow::Result;
use std::path::Path;
use tantivy::collector::{Count, TopDocs};
use tantivy::query::{BooleanQuery, Query, QueryParser};
use tantivy::schema::*;
use tantivy::{Index, IndexReader, IndexWriter, ReloadPolicy, TantivyDocument, Term};

pub struct SearchEngine {
    index: Index,
    reader: IndexReader,
    writer: IndexWriter,
    fields: Fields,
}

#[derive(Clone, Copy)]
pub struct Fields {
    pub doc_id: Field,
    pub path: Field,
    pub filename: Field,
    pub extension: Field,
    pub modified_at: Field,
    pub size: Field,
}

impl SearchEngine {
    pub fn open(path: &Path) -> Result<Self> {
        std::fs::create_dir_all(path)?;

        let mut schema_builder = Schema::builder();
        let doc_id = schema_builder.add_text_field("doc_id", STRING | STORED);
        let path_f = schema_builder.add_text_field("path", TEXT | STORED);
        let filename = schema_builder.add_text_field("filename", TEXT | STORED);
        let extension = schema_builder.add_text_field("extension", STRING | STORED);
        let modified_at = schema_builder.add_i64_field("modified_at", INDEXED | STORED);
        let size = schema_builder.add_u64_field("size", INDEXED | STORED);
        let schema = schema_builder.build();

        let index = if path.join("meta.json").exists() {
            Index::open_in_dir(path)?
        } else {
            Index::create_in_dir(path, schema.clone())?
        };

        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::OnCommitWithDelay)
            .try_into()?;
        let writer = index.writer(64 * 1024 * 1024)?;

        Ok(Self {
            index,
            reader,
            writer,
            fields: Fields { doc_id, path: path_f, filename, extension, modified_at, size },
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn add_or_replace(
        &mut self,
        doc_id: &str,
        path: &str,
        filename: &str,
        extension: &str,
        modified_at: i64,
        size: u64,
    ) -> Result<()> {
        self.writer.delete_term(Term::from_field_text(self.fields.doc_id, doc_id));

        let mut d = TantivyDocument::default();
        d.add_text(self.fields.doc_id, doc_id);
        d.add_text(self.fields.path, path);
        d.add_text(self.fields.filename, filename);
        d.add_text(self.fields.extension, extension);
        d.add_i64(self.fields.modified_at, modified_at);
        d.add_u64(self.fields.size, size);
        self.writer.add_document(d)?;
        Ok(())
    }

    pub fn remove(&mut self, doc_id: &str) -> Result<()> {
        self.writer.delete_term(Term::from_field_text(self.fields.doc_id, doc_id));
        Ok(())
    }

    pub fn commit(&mut self) -> Result<()> {
        self.writer.commit()?;
        self.reader.reload()?;
        Ok(())
    }

    pub fn search(
        &self,
        query_str: &str,
        limit: usize,
        folder_paths: Option<&std::collections::HashSet<String>>,
    ) -> Result<SearchResponse> {
        let started = std::time::Instant::now();
        let searcher = self.reader.searcher();

        let mut query_parser = QueryParser::for_index(
            &self.index,
            vec![self.fields.filename, self.fields.path],
        );
        query_parser.set_conjunction_by_default();

        let base_query: Box<dyn Query> = query_parser
            .parse_query(query_str)
            .unwrap_or_else(|_| {
                query_parser
                    .parse_query(&crate::search::query::escape_term(query_str))
                    .unwrap_or_else(|_| Box::new(BooleanQuery::new(vec![])))
            });

        let (top_docs, count) =
            searcher.search(&base_query, &(TopDocs::with_limit(limit), Count))?;

        let mut hits = Vec::with_capacity(top_docs.len());
        for (score, addr) in top_docs {
            let doc: TantivyDocument = searcher.doc(addr)?;
            let path: String = doc
                .get_first(self.fields.path)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            if let Some(set) = folder_paths {
                if !set.is_empty() && !set.iter().any(|p| crate::core::path_under(&path, p)) {
                    continue;
                }
            }

            let filename: String = doc
                .get_first(self.fields.filename)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let extension: String = doc
                .get_first(self.fields.extension)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let modified_at = doc
                .get_first(self.fields.modified_at)
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            let size = doc
                .get_first(self.fields.size)
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let doc_id: String = doc
                .get_first(self.fields.doc_id)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            hits.push(SearchHit {
                doc_id,
                path,
                filename,
                extension,
                size,
                modified_at,
                score,
            });
        }

        let hits_len = hits.len() as u64;
        let total = (count as u64).max(hits_len);
        Ok(SearchResponse {
            hits,
            total,
            elapsed_ms: started.elapsed().as_millis(),
        })
    }
}
