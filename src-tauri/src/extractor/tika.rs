use crate::core::ExtractedDocument;
use std::path::Path;
use std::time::Duration;

/// Optional Apache Tika Server endpoint, e.g. http://127.0.0.1:9998/tika
/// Used only for legacy OLE2 formats (doc, ppt, xls) that native Rust
/// extractors do not parse. If the server is unreachable the file is
/// reported as skipped so indexing continues.
pub fn extract(path: &Path) -> anyhow::Result<ExtractedDocument> {
    let endpoint = std::env::var("TIKA_URL").unwrap_or_else(|_| "http://127.0.0.1:9998/tika".into());

    let client = reqwest_blocking_client();
    let bytes = std::fs::read(path)?;
    let file_name = path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();

    let resp = client
        .put(&endpoint)
        .header("Accept", "text/plain")
        .header("X-Tika-OCRLanguage", "kor")
        .header(
            "Content-Disposition",
            format!("attachment; filename=\"{}\"", file_name),
        )
        .timeout(Duration::from_secs(120))
        .body(bytes)
        .send()?;

    if !resp.status().is_success() {
        anyhow::bail!("tika server returned {}", resp.status());
    }

    let content = resp.text()?;
    let title = path.file_stem().map(|s| s.to_string_lossy().to_string());

    Ok(ExtractedDocument {
        title,
        content,
        author: None,
    })
}

fn reqwest_blocking_client() -> reqwest::blocking::Client {
    reqwest::blocking::Client::new()
}
