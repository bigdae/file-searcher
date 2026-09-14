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
    scan_with_stop(
        folder,
        extra_excludes,
        &std::sync::atomic::AtomicBool::new(false),
    )
}

pub fn scan_with_stop(
    folder: &Path,
    extra_excludes: &HashSet<String>,
    stop: &std::sync::atomic::AtomicBool,
) -> Vec<ScannedFile> {
    let mut out = Vec::new();
    for entry in WalkDir::new(folder)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| !is_excluded(e.path(), extra_excludes))
    {
        if stop.load(std::sync::atomic::Ordering::Relaxed) {
            break;
        }
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

pub fn is_excluded(path: &Path, extra: &HashSet<String>) -> bool {
    path.components().any(|c| {
        let s = c.as_os_str().to_string_lossy();
        DEFAULT_EXCLUDES.contains(&s.as_ref()) || extra.contains(s.as_ref()) || s.starts_with("~$")
    })
}

pub fn file_exists(path: &str) -> bool {
    !matches!(fs::symlink_metadata(path), Err(error) if error.kind() == std::io::ErrorKind::NotFound)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn watcher_and_scan_exclude_nested_cache_and_temporary_files() {
        let extra = HashSet::new();
        assert!(is_excluded(
            Path::new("docs/node_modules/package/file.txt"),
            &extra
        ));
        assert!(is_excluded(Path::new("docs/~$budget.xlsx"), &extra));
        assert!(!is_excluded(Path::new("docs/budget.xlsx"), &extra));
    }

    #[test]
    fn cancelled_scan_returns_no_files() {
        let stop = std::sync::atomic::AtomicBool::new(true);
        assert!(scan_with_stop(
            Path::new(env!("CARGO_MANIFEST_DIR")),
            &HashSet::new(),
            &stop
        )
        .is_empty());
    }
}
