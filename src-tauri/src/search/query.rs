
use super::index::SearchEngine;

/// Parse a raw query string into structured terms.
/// Supports:
///   name:foo  → filename term
///   ext:pdf   → extension filter
///   path:proj → path term
///   "quoted phrase"
///   plain terms → all fields
pub struct ParsedQuery {
    pub filename_terms: Vec<String>,
    pub path_terms: Vec<String>,
    pub ext_filters: Vec<String>,
    pub general: Vec<String>,
    pub phrases: Vec<String>,
}

pub fn parse(raw: &str) -> ParsedQuery {
    let mut out = ParsedQuery {
        filename_terms: vec![],
        path_terms: vec![],
        ext_filters: vec![],
        general: vec![],
        phrases: vec![],
    };

    let mut rest = raw.to_string();
    // extract quoted phrases
    while let Some(start) = rest.find('"') {
        if let Some(len) = rest[start + 1..].find('"') {
            let phrase = rest[start + 1..start + 1 + len].to_string();
            if !phrase.trim().is_empty() {
                out.phrases.push(phrase);
            }
            rest = format!("{}{}", &rest[..start], &rest[start + len + 2..]);
        } else {
            rest = rest.replace('"', "");
            break;
        }
    }

    for token in rest.split_whitespace() {
        if let Some(v) = token.strip_prefix("name:") {
            if !v.is_empty() {
                out.filename_terms.push(v.to_lowercase());
            }
        } else if let Some(v) = token.strip_prefix("ext:") {
            if !v.is_empty() {
                out.ext_filters.push(v.to_lowercase().trim_start_matches('.').to_string());
            }
        } else if let Some(v) = token.strip_prefix("path:") {
            if !v.is_empty() {
                out.path_terms.push(v.to_lowercase());
            }
        } else {
            out.general.push(token.to_string());
        }
    }

    out
}

/// Search scope selected by the user.
/// - `filename`         : search only the filename (fast, name lookup)
/// - `filename_content` : search filename, path and content
pub fn to_tantivy_query(q: &ParsedQuery, scope: &str) -> String {
    let filename_only = scope == "filename";
    let mut clauses: Vec<String> = Vec::new();

    for t in &q.general {
        if filename_only {
            clauses.push(format!("filename:{}", escape(t)));
        } else {
            clauses.push(t.clone());
        }
    }
    for t in &q.phrases {
        if filename_only {
            clauses.push(format!("filename:\"{}\"", escape(t)));
        } else {
            clauses.push(format!("\"{}\"", escape(t)));
        }
    }
    for t in &q.filename_terms {
        clauses.push(format!("filename:{}", escape(t)));
    }
    for t in &q.path_terms {
        if filename_only {
            clauses.push(format!("filename:{}", escape(t)));
        } else {
            clauses.push(format!("path:{}", escape(t)));
        }
    }
    for t in &q.ext_filters {
        clauses.push(format!("extension:{}", escape(t)));
    }

    clauses.join(" AND ")
}

fn escape(s: &str) -> String {
    escape_term(s)
}

/// Escape tantivy query syntax special characters.
pub fn escape_term(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_whitespace() || "\"*?+-~^&|()!{}[]:".contains(c) || c == '\\' {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

impl SearchEngine {
    /// High-level search entry that combines parse + engine call.
    /// `scope` is "filename" or "filename_content".
    pub fn search_parsed(
        &self,
        raw: &str,
        limit: usize,
        folder_paths: &std::collections::HashSet<String>,
        scope: &str,
    ) -> anyhow::Result<crate::core::SearchResponse> {
        let parsed = parse(raw);
        let q = to_tantivy_query(&parsed, scope);
        self.search(&q, limit, Some(folder_paths))
    }
}
