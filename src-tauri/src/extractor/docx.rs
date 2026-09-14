use crate::core::ExtractedDocument;
use quick_xml::events::Event;
use quick_xml::Reader;
use std::fs::File;
use std::io::{BufReader, Read, Seek};
use std::path::Path;
use zip::ZipArchive;

/// Extract text from a zip-based OOXML part (`word/document.xml` etc).
/// Runs `<w:t>`-style text nodes together, treating every element boundary
/// as a soft break so words from separate runs do not concatenate.
fn read_part_text<R: Read + Seek>(archive: &mut ZipArchive<R>, part: &str) -> anyhow::Result<Option<String>> {
    let mut out = String::new();
    {
        let file = match archive.by_name(part) {
            Ok(f) => f,
            Err(zip::result::ZipError::FileNotFound) => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        let mut reader = Reader::from_reader(BufReader::new(file));
        reader.config_mut().trim_text(false);

        let mut buf = Vec::new();
        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Text(t)) => {
                    let s = t.unescape().unwrap_or_default().to_string();
                    out.push_str(&s);
                }
                Ok(Event::Start(_)) | Ok(Event::Empty(_)) | Ok(Event::End(_)) => {
                    out.push(' ');
                }
                Ok(Event::Eof) => break,
                Err(e) => anyhow::bail!("xml parse error in {part}: {e}"),
                _ => {}
            }
            buf.clear();
        }
    }
    let collapsed = out.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.trim().is_empty() {
        Ok(None)
    } else {
        Ok(Some(collapsed))
    }
}

pub fn extract(path: &Path) -> anyhow::Result<ExtractedDocument> {
    let file = File::open(path)?;
    let mut archive = ZipArchive::new(BufReader::new(file))?;

    let mut parts = Vec::new();
    for i in 0..archive.len() {
        let name = archive.by_index(i)?.name().to_string();
        if name.starts_with("word/") && name.ends_with(".xml") {
            parts.push(name);
        }
    }
    parts.sort();

    let mut content = String::new();
    for part in parts {
        if let Some(text) = read_part_text(&mut archive, &part)? {
            content.push_str(&text);
            content.push('\n');
        }
    }

    let title = path.file_stem().map(|s| s.to_string_lossy().to_string());

    Ok(ExtractedDocument {
        title,
        content,
        author: None,
    })
}
