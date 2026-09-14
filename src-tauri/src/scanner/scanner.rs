use crate::core::supported_extension;
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use walkdir::WalkDir;

pub const DEFAULT_EXCLUDES: &[&str] = &[
    "node_modules",
    ".git",
    "target",
    "dist",
    "build",
    ".cache",
    ".DS_Store",
];

#[derive(Debug, Clone, serde::Serialize)]
pub struct ScannedFile {
    pub path: String,
    pub size: u64,
    pub mtime: i64,
    pub extension: String,
}

pub fn scan(folder: &Path, extra_excludes: &HashSet<String>) -> Vec<ScannedFile> {
    let mut out = Vec::new();
    for entry in WalkDir::new(folder)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| !is_excluded(e.path(), extra_excludes))
    {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        if !entry.file_type().is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy();
        if name.starts_with("~$") || name == ".DS_Store" {
            continue;
        }
        let ext = entry
            .path()
            .extension()
            .map(|s| s.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        if !supported_extension(&ext) {
            continue;
        }
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        out.push(ScannedFile {
            path: entry.path().to_string_lossy().to_string(),
            size: meta.len(),
            mtime,
            extension: ext,
        });
    }
    out
}

fn is_excluded(path: &Path, extra: &HashSet<String>) -> bool {
    path.components().any(|c| {
        let s = c.as_os_str().to_string_lossy();
        DEFAULT_EXCLUDES.contains(&s.as_ref()) || extra.contains(s.as_ref())
    })
}

pub fn file_exists(path: &str) -> bool {
    fs::metadata(path).is_ok()
}
