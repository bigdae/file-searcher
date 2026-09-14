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
fn korean_search_end_to_end() -> Result<()> {
    // sample corpus
    let docs = temp_dir("docs");
    std::fs::write(docs.join("회의록_2026.md"), "# 2027년 스마트팩토리 구축 계획\n스마트팩토리 도입은 2027년부터 3단계로 진행한다.\n")?;
    std::fs::write(docs.join("계획.txt"), "MES 시스템과 연계한 생산관리 자동화.\n")?;
    std::fs::write(docs.join("data.csv"), "품목,수량\n볼트,1000\n")?;
    std::fs::write(docs.join("노트.txt"), "완전히 다른 내용이 들어 있다. AI 인공지능.\n")?;

    // engine + db in temp dirs
    let index_dir = temp_dir("idx");
    let db_dir = temp_dir("db");
    let mut engine = SearchEngine::open(&index_dir)?;
    let db = Db::open(&db_dir.join("index.db"))?;
    let folder_id = db.add_folder(docs.to_string_lossy().as_ref())?;

    // index all files (scanner + extractor + engine)
    let files = file_search_app::scanner::scanner::scan(&docs, &HashSet::new());
    assert!(files.len() >= 3);
    for f in &files {
        let doc = match file_search_app::extractor::extract(std::path::Path::new(&f.path)) {
            file_search_app::extractor::Extraction::Ok(d) => d,
            _ => panic!("extract failed"),
        };
        let doc_id = uuid::Uuid::new_v4().to_string();
        engine.add_or_replace(
            &doc_id,
            &f.path,
            &file_search_app::core::filename_of(&f.path),
            &f.extension,
            &doc.content,
            f.mtime,
            f.size,
        )?;
        db.upsert_file(folder_id, &f.path, f.size, f.mtime, &doc_id, "ok")?;
    }
    engine.commit()?;

    // search: filename+content scope
    let mut roots = HashSet::new();
    roots.insert(docs.to_string_lossy().to_string());
    let res = engine.search_parsed("스마트팩토리", 10, &roots, "filename_content")?;
    assert!(res.total >= 1, "스마트팩토리 hit expected");
    let hit = res.hits.iter().find(|h| h.filename.contains("회의록")).expect("회의록 hit");
    assert!(hit.snippet.contains("스마트팩토리"));

    // filename-only scope
    let res_name = engine.search_parsed("회의록", 10, &roots, "filename")?;
    assert!(res_name.total >= 1);

    // ext filter
    let res_ext = engine.search_parsed("품목 ext:csv", 10, &roots, "filename_content")?;
    assert!(res_ext.hits.iter().all(|h| h.extension == "csv"));
    assert!(res_ext.total >= 1);

    // phrase search
    let res_phrase = engine.search_parsed("\"스마트팩토리 구축\"", 10, &roots, "filename_content")?;
    assert!(res_phrase.total >= 1);

    // non-match
    let res_none = engine.search_parsed("존재하지않는검색어", 10, &roots, "filename_content")?;
    assert_eq!(res_none.total, 0);

    // english case-insensitivity
    let res_ai = engine.search_parsed("인공지능", 10, &roots, "filename_content")?;
    assert!(res_ai.total >= 1);

    Ok(())
}

#[test]
fn incremental_skip_unchanged() -> Result<()> {
    let docs = temp_dir("inc");
    std::fs::write(docs.join("a.txt"), "apple banana")?;

    let index_dir = temp_dir("idx2");
    let db_dir = temp_dir("db2");
    let mut engine = SearchEngine::open(&index_dir)?;
    let db = Db::open(&db_dir.join("index.db"))?;
    let folder_id = db.add_folder(docs.to_string_lossy().as_ref())?;

    let meta = std::fs::metadata(docs.join("a.txt"))?;
    let mtime = meta
        .modified()
        .unwrap()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    let doc_id = "d1".to_string();
    engine.add_or_replace(&doc_id, docs.join("a.txt").to_str().unwrap(), "a.txt", "txt", "apple banana", mtime, meta.len())?;
    engine.commit()?;
    db.upsert_file(folder_id, docs.join("a.txt").to_str().unwrap(), meta.len(), mtime, &doc_id, "ok")?;

    // unchanged → should be present in db with same mtime
    let stored = db.get_file(docs.join("a.txt").to_str().unwrap())?.unwrap();
    assert_eq!(stored.mtime, mtime);
    assert_eq!(stored.status, "ok");

    // delete → doc gone
    db.delete_file(docs.join("a.txt").to_str().unwrap())?;
    assert!(db.get_file(docs.join("a.txt").to_str().unwrap())?.is_none());

    Ok(())
}
