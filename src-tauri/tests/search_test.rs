use anyhow::Result;
use file_search_app::database::sqlite::Db;
use file_search_app::search::index::SearchEngine;
use std::collections::HashSet;

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("fs-test-{}-{}", tag, uuid::Uuid::new_v4()));
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
fn partial_word_search_matches_substrings() -> Result<()> {
    let mut engine = SearchEngine::open(&temp_dir("ngram-index"))?;
    engine.add_or_replace("1", "/files/abc.txt", "abc.txt", "txt", 1, 10)?;
    engine.add_or_replace("2", "/files/data.csv", "data.csv", "csv", 1, 10)?;
    engine.add_or_replace("3", "/files/회의록_2026.md", "회의록_2026.md", "md", 1, 10)?;
    engine.commit()?;

    // A single letter matches every filename that contains it, case-insensitive
    let single = engine.search_parsed("A", 10, &HashSet::new())?;
    let names: HashSet<&str> = single.hits.iter().map(|h| h.filename.as_str()).collect();
    assert!(
        names.contains("abc.txt") && names.contains("data.csv"),
        "{names:?}"
    );
    assert_eq!(single.total, 2);

    // inner substring, not just a token prefix
    let inner = engine.search_parsed("ta", 10, &HashSet::new())?;
    assert_eq!(inner.total, 1);
    assert_eq!(inner.hits[0].filename, "data.csv");

    // Korean partial words: 회의 must match 회의록
    let korean = engine.search_parsed("회의", 10, &HashSet::new())?;
    assert_eq!(korean.total, 1);
    assert!(korean.hits[0].filename.contains("회의"));
    Ok(())
}

#[test]
fn open_rebuilds_index_with_incompatible_older_schema() -> Result<()> {
    let index_dir = temp_dir("schema-mismatch");
    // Reproduce a 0.1.x index: same fields plus a `content` field that shifts
    // every later field ID.
    {
        use tantivy::schema::*;
        use tantivy::{Index, TantivyDocument};
        let mut builder = Schema::builder();
        let doc_id = builder.add_text_field("doc_id", STRING | STORED);
        let path_f = builder.add_text_field("path", TEXT | STORED);
        let filename = builder.add_text_field("filename", TEXT | STORED);
        let extension = builder.add_text_field("extension", STRING | STORED);
        let content = builder.add_text_field("content", TEXT | STORED);
        let modified_at = builder.add_i64_field("modified_at", INDEXED | STORED);
        let size = builder.add_u64_field("size", INDEXED | STORED);
        let schema = builder.build();
        let index = Index::create_in_dir(&index_dir, schema)?;
        let mut writer = index.writer(50_000_000)?;
        let mut doc = TantivyDocument::default();
        doc.add_text(doc_id, "old-doc");
        doc.add_text(path_f, "/old/report.txt");
        doc.add_text(filename, "report.txt");
        doc.add_text(extension, "txt");
        doc.add_text(content, "body");
        doc.add_i64(modified_at, 1);
        doc.add_u64(size, 10);
        writer.add_document(doc)?;
        writer.commit()?;
    }

    // Opening must not raise "expected a I64 for field modified_at"; the stale
    // index is rebuilt with the current schema and old documents are dropped.
    let mut engine = SearchEngine::open(&index_dir)?;
    engine.add_or_replace("new-doc", "/new/a.txt", "a.txt", "txt", 2, 20)?;
    engine.commit()?;
    assert!(engine.contains_document("new-doc", "/new/a.txt", 2, 20)?);
    assert_eq!(engine.search_parsed("a.txt", 10, &HashSet::new())?.total, 1);
    assert!(!engine.contains_document("old-doc", "/old/report.txt", 1, 10)?);
    assert_eq!(
        engine.search_parsed("report", 10, &HashSet::new())?.total,
        0
    );
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
    engine.add_or_replace(
        &doc_id,
        path.to_str().unwrap(),
        "a.txt",
        "txt",
        mtime,
        meta.len(),
    )?;
    engine.commit()?;
    db.upsert_file(
        folder_id,
        path.to_str().unwrap(),
        meta.len(),
        mtime,
        &doc_id,
        "ok",
    )?;

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
    assert!(!path_under(
        "/Users/me/ProjectsBackup/a.txt",
        "/Users/me/Projects"
    ));
    // trailing separator normalization
    assert!(path_under(
        "/Users/me/Projects/a.txt",
        "/Users/me/Projects/"
    ));
}

#[test]
fn folder_filter_applies_before_limit_and_total() -> Result<()> {
    let mut engine = SearchEngine::open(&temp_dir("scope-index"))?;
    for i in 0..12 {
        engine.add_or_replace(
            &format!("outside-{i}"),
            &format!("/other/report{i}.txt"),
            "report.txt",
            "txt",
            0,
            1,
        )?;
    }
    for i in 0..3 {
        engine.add_or_replace(
            &format!("inside-{i}"),
            &format!("/selected/report{i}.txt"),
            "report.txt",
            "txt",
            0,
            1,
        )?;
    }
    engine.commit()?;
    let roots = HashSet::from(["/selected".to_string()]);
    let response = engine.search_parsed("report", 2, &roots)?;
    assert_eq!(response.total, 3);
    assert_eq!(response.hits.len(), 2);
    assert!(response
        .hits
        .iter()
        .all(|hit| hit.path.starts_with("/selected/")));
    let zero = engine.search_parsed("report", 0, &roots)?;
    assert_eq!(zero.total, 3);
    assert!(zero.hits.is_empty());
    let unscoped_zero = engine.search_parsed("report", 0, &HashSet::new())?;
    assert_eq!(unscoped_zero.total, 15);
    assert!(unscoped_zero.hits.is_empty());
    Ok(())
}

#[test]
fn recursive_deletion_respects_folder_boundaries_and_literal_names() -> Result<()> {
    let db = Db::open(&temp_dir("delete-scope").join("index.db"))?;
    let folder_id = db.add_folder("/files")?;
    let paths = [
        "/files/a_%.dir/one.txt",
        "/files/a_%.dir/sub/two.txt",
        "/files/a_%.dir-backup/keep.txt",
        "/files/abX.dir/keep.txt",
    ];
    for (i, path) in paths.iter().enumerate() {
        db.upsert_file(folder_id, path, 1, 0, &format!("doc-{i}"), "ok")?;
    }
    let deleted = db.delete_files_under("/files/a_%.dir/")?;
    assert_eq!(deleted.len(), 2);
    assert!(db.get_file(paths[0])?.is_none());
    assert!(db.get_file(paths[1])?.is_none());
    assert!(db.get_file(paths[2])?.is_some());
    assert!(db.get_file(paths[3])?.is_some());
    Ok(())
}

#[test]
fn committed_document_check_detects_missing_and_stale_index() -> Result<()> {
    let index_dir = temp_dir("committed-index");
    let db = Db::open(&temp_dir("committed-db").join("index.db"))?;
    let folder_id = db.add_folder("/files")?;
    let path = "/files/report.txt";
    db.upsert_file(folder_id, path, 10, 1, "stable-id", "ok")?;
    let mut engine = SearchEngine::open(&index_dir)?;
    assert!(!engine.contains_document("stable-id", path, 1, 10)?);
    engine.add_or_replace("stable-id", path, "report.txt", "txt", 1, 10)?;
    assert!(!engine.contains_document("stable-id", path, 1, 10)?);
    engine.commit()?;
    assert!(engine.contains_document("stable-id", path, 1, 10)?);
    assert!(!engine.contains_document("stable-id", path, 2, 20)?);
    assert!(!engine.contains_document("stable-id", "/other/report.txt", 1, 10)?);
    engine.add_or_replace("stable-id", path, "report.txt", "txt", 2, 20)?;
    engine.commit()?;
    drop(engine);
    let engine = SearchEngine::open(&index_dir)?;
    assert!(engine.contains_document("stable-id", path, 2, 20)?);
    assert!(!engine.contains_document("stable-id", path, 1, 10)?);
    assert_eq!(
        engine.search_parsed("report", 10, &HashSet::new())?.total,
        1
    );
    // Rebuilding the index cannot trust an otherwise unchanged SQLite row.
    let rebuilt = SearchEngine::open(&temp_dir("rebuilt-index"))?;
    assert_eq!(db.get_file(path)?.unwrap().status, "ok");
    assert!(!rebuilt.contains_document("stable-id", path, 1, 10)?);
    Ok(())
}

#[test]
fn startup_pruning_removes_old_ids_and_preserves_current_documents() -> Result<()> {
    let mut engine = SearchEngine::open(&temp_dir("prune-index"))?;
    for id in ["old-id", "current-id", "orphan-id"] {
        engine.add_or_replace(id, "/files/report.txt", "report.txt", "txt", 1, 10)?;
    }
    engine.commit()?;
    let valid = HashSet::from(["current-id".to_string()]);
    assert_eq!(engine.prune_unknown_documents(&valid)?, 2);
    assert!(engine.contains_document("current-id", "/files/report.txt", 1, 10)?);
    assert_eq!(
        engine.search_parsed("report", 10, &HashSet::new())?.total,
        1
    );
    assert_eq!(engine.prune_unknown_documents(&valid)?, 0);
    assert_eq!(engine.prune_unknown_documents(&HashSet::new())?, 1);
    assert_eq!(
        engine.search_parsed("report", 10, &HashSet::new())?.total,
        0
    );
    Ok(())
}
