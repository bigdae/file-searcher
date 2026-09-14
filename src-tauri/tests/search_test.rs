use anyhow::Result;
use file_search_app::database::sqlite::Db;
use file_search_app::search::index::SearchEngine;
use std::collections::HashSet;

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "fs-test-{}-{}",
        tag,
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn filename_search_end_to_end() -> Result<()> {
    // sample corpus: filenames carry the searchable content
    let docs = temp_dir("docs");
    std::fs::write(docs.join("회의록_2026.md"), "body does not matter")?;
    std::fs::write(docs.join("스마트팩토리_계획.docx"), "x")?;
    std::fs::write(docs.join("data.csv"), "v")?;
    std::fs::write(docs.join("AI_보고서.txt"), "y")?;
    std::fs::create_dir_all(docs.join("node_modules"))?;
    std::fs::write(docs.join("node_modules/회의록.md"), "z")?;

    let index_dir = temp_dir("idx");
    let db_dir = temp_dir("db");
    let mut engine = SearchEngine::open(&index_dir)?;
    let db = Db::open(&db_dir.join("index.db"))?;
    let folder_id = db.add_folder(docs.to_string_lossy().as_ref())?;

    // index all files (scanner only, no extraction)
    let files = file_search_app::scanner::scanner::scan(&docs, &HashSet::new());
    // node_modules excluded
    assert_eq!(files.len(), 4, "node_modules should be excluded");
    for f in &files {
        let doc_id = uuid::Uuid::new_v4().to_string();
        engine.add_or_replace(
            &doc_id,
            &f.path,
            &file_search_app::core::filename_of(&f.path),
            &f.extension,
            f.mtime,
            f.size,
        )?;
        db.upsert_file(folder_id, &f.path, f.size, f.mtime, &doc_id, "ok")?;
    }
    engine.commit()?;

    let mut roots = HashSet::new();
    roots.insert(docs.to_string_lossy().to_string());

    // korean filename search
    let res = engine.search_parsed("회의록", 10, &roots)?;
    assert_eq!(res.total, 1, "only the top-level 회의록 file should match");
    let hit = &res.hits[0];
    assert!(hit.filename.contains("회의록"));

    // combined terms
    let res2 = engine.search_parsed("스마트팩토리 계획", 10, &roots)?;
    assert_eq!(res2.total, 1);

    // ext filter
    let res_ext = engine.search_parsed("보고서 ext:txt", 10, &roots)?;
    assert!(res_ext.hits.iter().all(|h| h.extension == "txt"));
    assert_eq!(res_ext.total, 1);

    // phrase search
    let res_phrase = engine.search_parsed("\"스마트팩토리_계획\"", 10, &roots)?;
    assert_eq!(res_phrase.total, 1);

    // path filter
    let res_path = engine.search_parsed("path:docs 회의록", 10, &roots)?;
    assert_eq!(res_path.total, 1);

    // non-match
    let res_none = engine.search_parsed("존재하지않는검색어", 10, &roots)?;
    assert_eq!(res_none.total, 0);

    Ok(())
}

#[test]
fn incremental_skip_unchanged() -> Result<()> {
    let docs = temp_dir("inc");
    let path = docs.join("a.txt");
    std::fs::write(&path, "apple banana")?;

    let index_dir = temp_dir("idx2");
    let db_dir = temp_dir("db2");
    let mut engine = SearchEngine::open(&index_dir)?;
    let db = Db::open(&db_dir.join("index.db"))?;
    let folder_id = db.add_folder(docs.to_string_lossy().as_ref())?;

    let meta = std::fs::metadata(&path)?;
    let mtime = meta
        .modified()
        .unwrap()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    let doc_id = "d1".to_string();
    engine.add_or_replace(&doc_id, path.to_str().unwrap(), "a.txt", "txt", mtime, meta.len())?;
    engine.commit()?;
    db.upsert_file(folder_id, path.to_str().unwrap(), meta.len(), mtime, &doc_id, "ok")?;

    let stored = db.get_file(path.to_str().unwrap())?.unwrap();
    assert_eq!(stored.mtime, mtime);
    assert_eq!(stored.status, "ok");

    db.delete_file(path.to_str().unwrap())?;
    assert!(db.get_file(path.to_str().unwrap())?.is_none());

    Ok(())
}

#[test]
fn path_under_matches_correctly() {
    use file_search_app::core::path_under;
    assert!(path_under("/Users/me/Projects/a.txt", "/Users/me/Projects"));
    assert!(path_under("/Users/me/Projects", "/Users/me/Projects"));
    assert!(!path_under("/Users/me/ProjectsBackup/a.txt", "/Users/me/Projects"));
    // trailing separator normalization
    assert!(path_under("/Users/me/Projects/a.txt", "/Users/me/Projects/"));
}
