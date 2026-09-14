use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchHit {
    pub doc_id: String,
    pub path: String,
    pub filename: String,
    pub extension: String,
    pub size: u64,
    pub modified_at: i64,
    pub score: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResponse {
    pub hits: Vec<SearchHit>,
    pub total: u64,
    pub elapsed_ms: u128,
}

pub fn filename_of(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string())
}

pub fn extension_of(path: &str) -> String {
    Path::new(path)
        .extension()
        .map(|s| s.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

/// True when `child` is inside `parent` (or equal). Normalizes separators
/// and compares case-insensitively on Windows so `D:\Projects` does not
/// match `D:\ProjectsBackup`.
pub fn path_under(child: &str, parent: &str) -> bool {
    let norm = |s: &str| -> String {
        let s = s.replace('/', std::path::MAIN_SEPARATOR_STR);
        let s = s.trim_end_matches(std::path::MAIN_SEPARATOR);
        #[cfg(target_os = "windows")]
        let s = s.to_lowercase();
        s.to_string()
    };
    let parent = norm(parent);
    let child = norm(child);
    child == parent || child.starts_with(&format!("{parent}{}", std::path::MAIN_SEPARATOR))
}
