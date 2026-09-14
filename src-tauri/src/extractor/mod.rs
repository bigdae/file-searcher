pub mod docx;
pub mod pdf;
pub mod pptx;
pub mod tika;
pub mod text;
pub mod xlsx;

use crate::core::ExtractedDocument;
use std::path::Path;

pub enum Extraction {
    Ok(ExtractedDocument),
    Skipped(String),
    Failed(String),
}

pub fn extract(path: &Path) -> Extraction {
    let ext = path
        .extension()
        .map(|s| s.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();

    let result = match ext.as_str() {
        "txt" | "md" | "csv" | "json" | "xml" | "html" | "htm" => text::extract(path),
        "docx" => docx::extract(path),
        "pptx" => pptx::extract(path),
        "xlsx" => xlsx::extract(path),
        "pdf" => pdf::extract(path),
        "doc" | "ppt" | "xls" => tika::extract(path),
        _ => return Extraction::Skipped(format!("unsupported extension: {ext}")),
    };

    match result {
        Ok(doc) => Extraction::Ok(doc),
        Err(e) => Extraction::Failed(format!("{ext}: {e}")),
    }
}
