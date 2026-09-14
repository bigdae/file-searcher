use crate::core::{SearchHit, SearchResponse};
use anyhow::Result;
use std::path::Path;
use tantivy::collector::{Count, TopDocs};
use tantivy::query::{BooleanQuery, Query, QueryParser};
use tantivy::schema::*;
use tantivy::tokenizer::{LowerCaser, NgramTokenizer, TextAnalyzer};
use tantivy::{Index, IndexReader, IndexWriter, ReloadPolicy, TantivyDocument, Term};

/// Tokenizer name for the `filename` field: 1–3 character grams so that
/// partial words (e.g. "A" or "회의") match files whose names merely contain
/// them. A bare word that analyzes into multiple tokens automatically becomes
/// a tantivy `PhraseQuery`, which is what makes substring search work.
const FILENAME_TOKENIZER: &str = "ngram";

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

/// True when the on-disk schema is exactly the schema this build expects:
/// same field order, names, and types. Field IDs are positional, so anything
/// else must trigger a rebuild.
fn schema_matches(existing: &Schema, expected: &Schema) -> bool {
    let existing: Vec<&FieldEntry> = existing.fields().map(|(_, e)| e).collect();
    let expected: Vec<&FieldEntry> = expected.fields().map(|(_, e)| e).collect();
    existing.len() == expected.len()
        && existing.iter().zip(&expected).all(|(a, b)| {
            a.name() == b.name()
                && format!("{:?}", a.field_type()) == format!("{:?}", b.field_type())
        })
}

/// Delete every file/subdirectory inside `path` (keeps the directory itself).
fn clear_dir(path: &Path) -> Result<()> {
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        let child = entry.path();
        if child.is_dir() {
            std::fs::remove_dir_all(child)?;
        } else {
            std::fs::remove_file(child)?;
        }
    }
    Ok(())
}

impl SearchEngine {
    pub fn open(path: &Path) -> Result<Self> {
        std::fs::create_dir_all(path)?;

        let mut schema_builder = Schema::builder();
        let doc_id = schema_builder.add_text_field("doc_id", STRING | STORED);
        let path_f = schema_builder.add_text_field("path", TEXT | STORED);
        let filename = schema_builder.add_text_field(
            "filename",
            TextOptions::default()
                .set_indexing_options(
                    TextFieldIndexing::default()
                        .set_tokenizer(FILENAME_TOKENIZER)
                        .set_index_option(IndexRecordOption::WithFreqsAndPositions),
                )
                .set_stored(),
        );
        let extension = schema_builder.add_text_field("extension", STRING | STORED);
        let modified_at = schema_builder.add_i64_field("modified_at", INDEXED | STORED);
        let size = schema_builder.add_u64_field("size", INDEXED | STORED);
        let schema = schema_builder.build();

        let index = if path.join("meta.json").exists() {
            let index = Index::open_in_dir(path)?;
            if schema_matches(&index.schema(), &schema) {
                index
            } else {
                // Field IDs are positional. Opening an index built by an older
                // release (e.g. 0.1.x with a `content` field) but writing with
                // the current field IDs corrupts every document and fails with
                // "Schema error: expected a I64 for field modified_at". Rebuild
                // from scratch; startup reconciliation reindexes all files.
                log::warn!("index schema mismatch — rebuilding {}", path.display());
                clear_dir(path)?;
                Index::create_in_dir(path, schema.clone())?
            }
        } else {
            Index::create_in_dir(path, schema.clone())?
        };

        let ngram = TextAnalyzer::builder(NgramTokenizer::new(1, 3, false)?)
            .filter(LowerCaser)
            .build();
        index.tokenizers().register(FILENAME_TOKENIZER, ngram);

        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::OnCommitWithDelay)
            .try_into()?;
        let writer = index.writer(64 * 1024 * 1024)?;

        Ok(Self {
            index,
            reader,
            writer,
            fields: Fields {
                doc_id,
                path: path_f,
                filename,
                extension,
                modified_at,
                size,
            },
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
        self.writer
            .delete_term(Term::from_field_text(self.fields.doc_id, doc_id));

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
        self.writer
            .delete_term(Term::from_field_text(self.fields.doc_id, doc_id));
        Ok(())
    }

    pub fn commit(&mut self) -> Result<()> {
        self.writer.commit()?;
        self.reader.reload()?;
        Ok(())
    }

    /// Check committed data before trusting SQLite's incremental-index marker.
    pub fn contains_document(
        &self,
        doc_id: &str,
        path: &str,
        modified_at: i64,
        size: u64,
    ) -> Result<bool> {
        let searcher = self.reader.searcher();
        let query = tantivy::query::TermQuery::new(
            Term::from_field_text(self.fields.doc_id, doc_id),
            IndexRecordOption::Basic,
        );
        let (docs, count) = searcher.search(&query, &(TopDocs::with_limit(1), Count))?;
        if count != 1 {
            return Ok(false);
        }
        let doc: TantivyDocument = searcher.doc(docs[0].1)?;
        Ok(
            doc.get_first(self.fields.path).and_then(|v| v.as_str()) == Some(path)
                && doc
                    .get_first(self.fields.modified_at)
                    .and_then(|v| v.as_i64())
                    == Some(modified_at)
                && doc.get_first(self.fields.size).and_then(|v| v.as_u64()) == Some(size),
        )
    }

    /// Remove obsolete UUIDs left behind by older indexing runs or interrupted deletions.
    pub fn prune_unknown_documents(
        &mut self,
        valid_doc_ids: &std::collections::HashSet<String>,
    ) -> Result<usize> {
        let searcher = self.reader.searcher();
        let addresses = searcher.search(
            &tantivy::query::AllQuery,
            &tantivy::collector::DocSetCollector,
        )?;
        let mut removed = 0;
        for address in addresses {
            let doc: TantivyDocument = searcher.doc(address)?;
            if let Some(doc_id) = doc.get_first(self.fields.doc_id).and_then(|v| v.as_str()) {
                if !valid_doc_ids.contains(doc_id) {
                    self.writer
                        .delete_term(Term::from_field_text(self.fields.doc_id, doc_id));
                    removed += 1;
                }
            }
        }
        if removed > 0 {
            self.commit()?;
        }
        Ok(removed)
    }

    pub fn search(
        &self,
        query_str: &str,
        limit: usize,
        folder_paths: Option<&std::collections::HashSet<String>>,
    ) -> Result<SearchResponse> {
        let started = std::time::Instant::now();
        let searcher = self.reader.searcher();

        let mut query_parser =
            QueryParser::for_index(&self.index, vec![self.fields.filename, self.fields.path]);
        query_parser.set_conjunction_by_default();

        let base_query: Box<dyn Query> = query_parser.parse_query(query_str).unwrap_or_else(|_| {
            query_parser
                .parse_query(&crate::search::query::escape_term(query_str))
                .unwrap_or_else(|_| Box::new(BooleanQuery::new(vec![])))
        });

        let scoped = folder_paths.is_some_and(|paths| !paths.is_empty());
        // Folder membership is stored in the document, so it must be checked
        // before applying the result limit or computing the scoped total.
        let collection_limit = if scoped {
            searcher.num_docs() as usize
        } else {
            limit
        };
        let (top_docs, count) = searcher.search(
            &base_query,
            &(TopDocs::with_limit(collection_limit.max(1)), Count),
        )?;

        let mut hits = Vec::with_capacity(top_docs.len().min(limit));
        let mut scoped_count = 0u64;
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

            scoped_count += 1;
            if hits.len() >= limit {
                continue;
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

        let total = if scoped { scoped_count } else { count as u64 };
        Ok(SearchResponse {
            hits,
            total,
            elapsed_ms: started.elapsed().as_millis(),
        })
    }
}
