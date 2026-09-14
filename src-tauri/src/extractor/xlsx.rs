use crate::core::ExtractedDocument;
use quick_xml::events::Event;
use quick_xml::Reader;
use std::fs::File;
use std::io::{BufReader, Read, Seek};
use std::path::Path;
use zip::ZipArchive;

fn parse_shared_strings<R: Read + Seek>(archive: &mut ZipArchive<R>) -> anyhow::Result<Vec<String>> {
    let mut strings = Vec::new();
    let file = match archive.by_name("xl/sharedStrings.xml") {
        Ok(f) => f,
        Err(zip::result::ZipError::FileNotFound) => return Ok(strings),
        Err(e) => return Err(e.into()),
    };
    let mut reader = Reader::from_reader(BufReader::new(file));
    let mut buf = Vec::new();
    let mut in_t = false;
    let mut current = String::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) if e.name().as_ref() == b"t" => {
                in_t = true;
                current.clear();
            }
            Ok(Event::Text(t)) if in_t => {
                current.push_str(&t.unescape().unwrap_or_default().to_string());
            }
            Ok(Event::End(e)) if e.name().as_ref() == b"t" => {
                strings.push(current.clone());
                in_t = false;
            }
            Ok(Event::Eof) => break,
            Err(e) => anyhow::bail!("sharedStrings parse error: {e}"),
            _ => {}
        }
        buf.clear();
    }
    Ok(strings)
}

fn collect_sheet_text<R: Read + Seek>(archive: &mut ZipArchive<R>, part: &str, shared: &[String]) -> anyhow::Result<String> {
    let mut out = Vec::new();
    let file = archive.by_name(part)?;
    let mut reader = Reader::from_reader(BufReader::new(file));
    let mut buf = Vec::new();
    let mut in_value = false;
    let mut cell_is_shared = false;
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                match e.name().as_ref() {
                    b"c" => {
                        cell_is_shared = e
                            .attributes()
                            .flatten()
                            .any(|a| a.key.as_ref() == b"t" && a.value.as_ref() == b"s");
                    }
                    b"v" | b"is" => in_value = true,
                    _ => {}
                }
            }
            Ok(Event::Text(t)) if in_value => {
                let raw = t.unescape().unwrap_or_default().to_string();
                if cell_is_shared {
                    if let Ok(idx) = raw.trim().parse::<usize>() {
                        if let Some(s) = shared.get(idx) {
                            out.push(s.clone());
                        }
                    }
                } else {
                    out.push(raw);
                }
            }
            Ok(Event::End(e)) => match e.name().as_ref() {
                b"v" | b"is" => in_value = false,
                b"c" => cell_is_shared = false,
                _ => {}
            },
            Ok(Event::Eof) => break,
            Err(e) => anyhow::bail!("sheet parse error in {part}: {e}"),
            _ => {}
        }
        buf.clear();
    }
    Ok(out.join(" "))
}

pub fn extract(path: &Path) -> anyhow::Result<ExtractedDocument> {
    let file = File::open(path)?;
    let mut archive = ZipArchive::new(BufReader::new(file))?;
    let shared = parse_shared_strings(&mut archive)?;

    let mut sheets = Vec::new();
    for i in 0..archive.len() {
        let name = archive.by_index(i)?.name().to_string();
        if name.starts_with("xl/worksheets/") && name.ends_with(".xml") {
            sheets.push(name);
        }
    }
    sheets.sort();

    let mut content = String::new();
    for part in sheets {
        let text = collect_sheet_text(&mut archive, &part, &shared)?;
        if !text.is_empty() {
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
