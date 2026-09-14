use crate::core::ExtractedDocument;
use encoding_rs::UTF_8;
use std::fs;
use std::io::Read;
use std::path::Path;

const MAX_TEXT_SIZE: u64 = 20 * 1024 * 1024;

pub fn extract(path: &Path) -> anyhow::Result<ExtractedDocument> {
    let meta = fs::metadata(path)?;
    if meta.len() > MAX_TEXT_SIZE {
        anyhow::bail!("file too large: {} bytes", meta.len());
    }

    let mut bytes = Vec::with_capacity(meta.len() as usize);
    fs::File::open(path)?.read_to_end(&mut bytes)?;

    let (text, _, had_errors) = UTF_8.decode(&bytes);
    let content = if had_errors {
        // fall back to lossy latin1 for legacy files
        String::from_utf8_lossy(&bytes).to_string()
    } else {
        text.into_owned()
    };

    let title = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string());

    Ok(ExtractedDocument {
        title,
        content,
        author: None,
    })
}
