use crate::core::ExtractedDocument;
use quick_xml::events::Event;
use quick_xml::Reader;
use std::fs::File;
use std::io::{BufReader, Read, Seek};
use std::path::Path;
use zip::ZipArchive;

fn slide_number(name: &str) -> Option<u32> {
    name.strip_prefix("ppt/slides/slide")?
        .strip_suffix(".xml")?
        .parse()
        .ok()
}

fn read_slide_text<R: Read + Seek>(archive: &mut ZipArchive<R>, part: &str) -> anyhow::Result<String> {
    let mut out = String::new();
    let file = archive.by_name(part)?;
    let mut reader = Reader::from_reader(BufReader::new(file));
    reader.config_mut().trim_text(false);

    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Text(t)) => {
                let s = t.unescape().unwrap_or_default().to_string();
                out.push_str(&s);
                out.push(' ');
            }
            Ok(Event::End(_)) => out.push('\n'),
            Ok(Event::Eof) => break,
            Err(e) => anyhow::bail!("xml parse error in {part}: {e}"),
            _ => {}
        }
        buf.clear();
    }
    Ok(out.split_whitespace().collect::<Vec<_>>().join(" "))
}

pub fn extract(path: &Path) -> anyhow::Result<ExtractedDocument> {
    let file = File::open(path)?;
    let mut archive = ZipArchive::new(BufReader::new(file))?;

    let mut slides: Vec<(u32, String)> = Vec::new();
    for i in 0..archive.len() {
        let name = archive.by_index(i)?.name().to_string();
        if let Some(n) = slide_number(&name) {
            slides.push((n, name));
        }
    }
    slides.sort_by_key(|(n, _)| *n);

    let mut content = String::new();
    for (_, part) in slides {
        let text = read_slide_text(&mut archive, &part)?;
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
