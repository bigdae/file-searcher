use crate::core::ExtractedDocument;
use std::path::Path;

const MAX_PDF_SIZE: u64 = 200 * 1024 * 1024;

pub fn extract(path: &Path) -> anyhow::Result<ExtractedDocument> {
    let meta = std::fs::metadata(path)?;
    if meta.len() > MAX_PDF_SIZE {
        anyhow::bail!("pdf too large: {} bytes", meta.len());
    }

    let text = pdf_extract::extract_text(path)?;
    let content = text.split_whitespace().collect::<Vec<_>>().join(" ");

    let title = path.file_stem().map(|s| s.to_string_lossy().to_string());

    Ok(ExtractedDocument {
        title,
        content,
        author: None,
    })
}
