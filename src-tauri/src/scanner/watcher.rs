use crate::database::sqlite::Db;
use crate::search::index::SearchEngine;
use anyhow::Result;
use notify::Watcher;
use notify_debouncer_full::{new_debouncer, DebouncedEvent, Debouncer, FileIdMap};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::Emitter;

pub struct AppState {
    pub db: Db,
    pub engine: Mutex<SearchEngine>,
    pub operation: Mutex<()>,
    pub busy: AtomicBool,
    pub stop_flag: AtomicBool,
    pub watcher: Mutex<Option<Debouncer<notify::RecommendedWatcher, FileIdMap>>>,
}

pub struct AppStateShared {
    pub inner: Arc<AppState>,
}

impl AppStateShared {
    pub fn new(db: Db, engine: SearchEngine) -> Self {
        Self {
            inner: Arc::new(AppState {
                db,
                engine: Mutex::new(engine),
                operation: Mutex::new(()),
                busy: AtomicBool::new(false),
                stop_flag: AtomicBool::new(false),
                watcher: Mutex::new(None),
            }),
        }
    }
}

pub fn emit_status(
    app: &tauri::AppHandle,
    state: &AppState,
    message: &str,
    progress: Option<(usize, usize)>,
) {
    let _ = app.emit(
        "index-status",
        serde_json::json!({
            "busy": state.busy.load(Ordering::Relaxed),
            "message": message,
            "done": progress.map(|p| p.0),
            "total": progress.map(|p| p.1),
        }),
    );
}

pub struct IndexOutcome {
    pub indexed: usize,
    pub skipped: usize,
    pub removed: usize,
}

/// Full or incremental indexing of one folder.
/// `incremental` skips files whose (size, mtime) match the DB record.
pub fn index_folder(
    app: &tauri::AppHandle,
    state: &AppState,
    folder_id: i64,
    folder_path: &str,
    incremental: bool,
) -> Result<IndexOutcome> {
    let _operation = state.operation.lock().unwrap();
    if !state
        .db
        .list_folders()?
        .iter()
        .any(|f| f.id == folder_id && f.enabled)
    {
        anyhow::bail!("인덱싱할 폴더가 삭제되었거나 비활성화되었습니다.");
    }
    state.busy.store(true, Ordering::Relaxed);
    let result = index_folder_inner(Some(app), state, folder_id, folder_path, incremental, true);
    state.busy.store(false, Ordering::Relaxed);
    match &result {
        Ok(_) if state.stop_flag.load(Ordering::Relaxed) => {
            emit_status(app, state, "인덱싱 중지됨", None)
        }
        Ok(_) => emit_status(app, state, "인덱싱 완료", None),
        Err(error) => {
            let _ = state.db.record_error(folder_path, &error.to_string());
            emit_status(app, state, &format!("인덱싱 실패: {error}"), None);
        }
    }
    result
}

fn index_folder_inner(
    app: Option<&tauri::AppHandle>,
    state: &AppState,
    folder_id: i64,
    folder_path: &str,
    incremental: bool,
    honor_stop: bool,
) -> Result<IndexOutcome> {
    let never_stop = AtomicBool::new(false);
    let stop = if honor_stop {
        &state.stop_flag
    } else {
        &never_stop
    };
    std::fs::read_dir(folder_path)?;
    let excludes: HashSet<String> = HashSet::new();
    if let Some(app) = app {
        emit_status(app, state, "파일 검색 중…", None);
    }
    let files = crate::scanner::scanner::scan_with_stop(Path::new(folder_path), &excludes, stop);

    let total = files.len();
    let mut indexed = 0usize;
    let mut skipped = 0usize;
    let mut removed = 0usize;

    // collect DB paths under this folder to detect deletions
    let mut known: HashMap<String, ()> = HashMap::new();
    {
        for (p, _) in state.db.all_files()? {
            if crate::core::path_under(&p, folder_path) {
                known.insert(p, ());
            }
        }
    }

    const COMMIT_BATCH: usize = 500;
    let mut pending = 0usize;
    let mut completed = Vec::new();

    if let Some(app) = app {
        emit_status(
            app,
            state,
            &format!("스캔 완료: {total}개 파일"),
            Some((0, total)),
        );
    }

    for (i, f) in files.iter().enumerate() {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        known.remove(&f.path);

        if incremental {
            if let Some(meta) = state.db.get_file(&f.path)? {
                let committed = match state.db.doc_id_of(&f.path)? {
                    Some(doc_id) => state
                        .engine
                        .lock()
                        .unwrap()
                        .contains_document(&doc_id, &f.path, f.mtime, f.size)?,
                    None => false,
                };
                if meta.size == f.size && meta.mtime == f.mtime && meta.status == "ok" && committed
                {
                    skipped += 1;
                    continue;
                }
            }
        }

        let doc_id = state
            .db
            .doc_id_of(&f.path)?
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        state
            .db
            .upsert_file(folder_id, &f.path, f.size, f.mtime, &doc_id, "pending")?;
        {
            let mut engine = state.engine.lock().unwrap();
            engine.add_or_replace(
                &doc_id,
                &f.path,
                &crate::core::filename_of(&f.path),
                &f.extension,
                f.mtime,
                f.size,
            )?;
            // commit in batches: per-file commit creates a new segment and
            // fsyncs meta.json every time, which stalls at tens of thousands
            // of files
            pending += 1;
            if pending >= COMMIT_BATCH {
                engine.commit()?;
                pending = 0;
            }
        }
        completed.push((f, doc_id));
        indexed += 1;

        if let Some(app) = app.filter(|_| i % 25 == 0) {
            emit_status(
                app,
                state,
                &format!("인덱싱 중… {}/{}", i + 1, total),
                Some((i + 1, total)),
            );
        }
    }

    // remove files that no longer exist on disk
    for gone in known.keys() {
        if !crate::scanner::scanner::file_exists(gone) {
            if let Some(doc_id) = state.db.delete_file(gone)? {
                state.engine.lock().unwrap().remove(&doc_id)?;
                removed += 1;
            }
        }
    }
    state.engine.lock().unwrap().commit()?;
    for (f, doc_id) in completed {
        state
            .db
            .upsert_file(folder_id, &f.path, f.size, f.mtime, &doc_id, "ok")?;
    }

    if !stop.load(Ordering::Relaxed) {
        state.db.touch_folder(folder_id)?;
    }
    Ok(IndexOutcome {
        indexed,
        skipped,
        removed,
    })
}

/// Start watching a folder with debounce; events trigger incremental re-index
/// of the affected files.
pub fn start_watcher(
    app: tauri::AppHandle,
    state: &Arc<AppState>,
    folders: Vec<PathBuf>,
) -> Result<()> {
    let mut watcher = state.watcher.lock().unwrap();
    let folders: HashSet<PathBuf> = folders
        .into_iter()
        .chain(all_watched_roots(state))
        .collect();
    let app = Arc::new(app);
    let state_for_cb = Arc::clone(state);

    let mut debouncer = new_debouncer(
        Duration::from_millis(1500),
        None,
        move |events: notify_debouncer_full::DebounceEventResult| {
            if let Ok(events) = events {
                handle_events(&app, &state_for_cb, events);
            }
        },
    )?;

    for folder in &folders {
        if let Err(error) = debouncer
            .watcher()
            .watch(folder, notify::RecursiveMode::Recursive)
        {
            log::warn!(
                "watch registration failed for {}: {error}",
                folder.display()
            );
        }
    }

    *watcher = Some(debouncer);
    Ok(())
}

#[allow(dead_code)]
pub fn stop_watcher(state: &AppState) {
    *state.watcher.lock().unwrap() = None;
}

/// Watch a folder added at runtime (debouncer is shared with the startup one).
pub fn add_watch(state: &AppState, folder: &Path) -> Result<()> {
    if let Some(deb) = state.watcher.lock().unwrap().as_mut() {
        deb.watcher()
            .watch(folder, notify::RecursiveMode::Recursive)?;
    }
    Ok(())
}

/// Stop watching a removed folder.
pub fn remove_watch(state: &AppState, folder: &Path) -> Result<()> {
    if let Some(deb) = state.watcher.lock().unwrap().as_mut() {
        deb.watcher().unwatch(folder)?;
    }
    Ok(())
}

fn handle_events(app: &tauri::AppHandle, state: &Arc<AppState>, events: Vec<DebouncedEvent>) {
    let _operation = state.operation.lock().unwrap();
    state.busy.store(true, Ordering::Relaxed);

    let mut seen: HashSet<String> = HashSet::new();
    let mut processed = false;
    let mut failure = None;
    for ev in &events {
        for path in &ev.paths {
            let p = path.to_string_lossy().to_string();
            if seen.insert(p.clone()) {
                if let Err(error) = process_path(Some(app), state, path) {
                    log::warn!("watch update failed for {}: {error}", path.display());
                    let _ = state.db.record_error(&p, &error.to_string());
                    failure = Some(error.to_string());
                }
                processed = true;
            }
        }
    }
    if processed {
        if let Err(error) = state.engine.lock().unwrap().commit() {
            failure = Some(error.to_string());
        }
    }

    state.busy.store(false, Ordering::Relaxed);
    match failure {
        Some(error) => emit_status(app, state, &format!("변경 반영 실패: {error}"), None),
        None => emit_status(app, state, "변경 반영 완료", None),
    }
}

fn process_path(app: Option<&tauri::AppHandle>, state: &AppState, path: &Path) -> Result<()> {
    let path_str = path.to_string_lossy().to_string();

    let folders = state.db.list_folders()?;
    let Some(folder_id) = folders
        .iter()
        .filter(|f| f.enabled && crate::core::path_under(&path_str, &f.path))
        .max_by_key(|f| f.path.len())
        .map(|f| f.id)
    else {
        return Ok(());
    };
    if crate::scanner::scanner::is_excluded(path, &HashSet::new()) {
        return Ok(());
    }

    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => Some(metadata),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };
    let Some(meta) = metadata else {
        for (_, doc_id) in state.db.delete_files_under(&path_str)? {
            state.engine.lock().unwrap().remove(&doc_id)?;
        }
        return Ok(());
    };

    if meta.file_type().is_symlink() {
        return Ok(());
    }

    if meta.is_dir() {
        // folder moved in: index it
        index_folder_inner(app, state, folder_id, &path_str, true, false)?;
        return Ok(());
    }

    if !meta.is_file() {
        return Ok(());
    }

    let ext = crate::core::extension_of(&path_str);

    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    // existing doc?
    if let Some(existing) = state.db.get_file(&path_str)? {
        let committed = match state.db.doc_id_of(&path_str)? {
            Some(doc_id) => state.engine.lock().unwrap().contains_document(
                &doc_id,
                &path_str,
                mtime,
                meta.len(),
            )?,
            None => false,
        };
        if existing.size == meta.len()
            && existing.mtime == mtime
            && existing.status == "ok"
            && committed
        {
            return Ok(());
        }
        // remove old doc
        if let Some(doc_id) = state.db.doc_id_of(&path_str)? {
            state.engine.lock().unwrap().remove(&doc_id)?;
        }
    }

    let doc_id = state
        .db
        .doc_id_of(&path_str)?
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    state
        .db
        .upsert_file(folder_id, &path_str, meta.len(), mtime, &doc_id, "pending")?;
    state.engine.lock().unwrap().add_or_replace(
        &doc_id,
        &path_str,
        &crate::core::filename_of(&path_str),
        &ext,
        mtime,
        meta.len(),
    )?;
    state.engine.lock().unwrap().commit()?;
    state
        .db
        .upsert_file(folder_id, &path_str, meta.len(), mtime, &doc_id, "ok")?;

    if let Some(app) = app {
        emit_status(
            app,
            state,
            &format!("갱신: {}", crate::core::filename_of(&path_str)),
            None,
        );
    }
    Ok(())
}

/// Startup reconciliation: re-check metadata of known files and pick up
/// changes that happened while the app was closed.
pub fn reconcile_on_startup(app: &tauri::AppHandle, state: &AppState) -> Result<IndexOutcome> {
    let folders = state.db.list_folders()?;
    let mut total = IndexOutcome {
        indexed: 0,
        skipped: 0,
        removed: 0,
    };
    let mut failed = false;

    for folder in &folders {
        if state.stop_flag.load(Ordering::Relaxed) {
            break;
        }
        if !folder.enabled {
            continue;
        }
        if !Path::new(&folder.path).exists() {
            continue;
        }
        match index_folder(app, state, folder.id, &folder.path, true) {
            Ok(outcome) => {
                total.indexed += outcome.indexed;
                total.skipped += outcome.skipped;
                total.removed += outcome.removed;
            }
            Err(e) => {
                failed = true;
                log::warn!("reconcile failed for {}: {e}", folder.path);
            }
        }
    }
    if !failed && !state.stop_flag.load(Ordering::Relaxed) {
        emit_status(app, state, "시작 정합성 확인 완료", None);
    }
    Ok(total)
}

/// Walk all watched roots (used to ensure watcher covers every folder).
pub fn all_watched_roots(state: &AppState) -> Vec<PathBuf> {
    state
        .db
        .list_folders()
        .unwrap_or_default()
        .into_iter()
        .filter(|f| f.enabled)
        .map(|f| PathBuf::from(f.path))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        state: AppStateShared,
        root: PathBuf,
        base: PathBuf,
        id: i64,
    }
    impl Fixture {
        fn new() -> Self {
            let base =
                std::env::temp_dir().join(format!("searcher-watcher-{}", uuid::Uuid::new_v4()));
            let root = base.join("documents");
            std::fs::create_dir_all(&root).unwrap();
            let state = AppStateShared::new(
                Db::open(&base.join("db.sqlite")).unwrap(),
                SearchEngine::open(&base.join("index")).unwrap(),
            );
            let id = state.inner.db.add_folder(root.to_str().unwrap()).unwrap();
            Self {
                state,
                root,
                base,
                id,
            }
        }
        fn scan(&self, incremental: bool) -> IndexOutcome {
            index_folder_inner(
                None,
                &self.state.inner,
                self.id,
                self.root.to_str().unwrap(),
                incremental,
                true,
            )
            .unwrap()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.base);
        }
    }

    #[test]
    fn repeated_index_and_uncommitted_metadata_recover_without_duplicates() {
        let f = Fixture::new();
        let path = f.root.join("report.txt");
        std::fs::write(&path, "first").unwrap();
        assert_eq!(f.scan(false).indexed, 1);
        assert_eq!(f.scan(false).indexed, 1);
        assert_eq!(f.scan(true).skipped, 1);
        assert_eq!(
            f.state
                .inner
                .engine
                .lock()
                .unwrap()
                .search("report", 20, None)
                .unwrap()
                .total,
            1
        );
        let doc_id = f
            .state
            .inner
            .db
            .doc_id_of(path.to_str().unwrap())
            .unwrap()
            .unwrap();
        {
            let mut engine = f.state.inner.engine.lock().unwrap();
            engine.remove(&doc_id).unwrap();
            engine.commit().unwrap();
        }
        assert_eq!(f.scan(true).indexed, 1);
        assert_eq!(
            f.state
                .inner
                .engine
                .lock()
                .unwrap()
                .search("report", 20, None)
                .unwrap()
                .total,
            1
        );
        std::fs::write(&path, "changed content").unwrap();
        assert_eq!(f.scan(true).indexed, 1);
        assert_eq!(
            f.state
                .inner
                .db
                .get_file(path.to_str().unwrap())
                .unwrap()
                .unwrap()
                .size,
            15
        );
    }

    #[test]
    fn directory_events_preserve_root_ownership_and_remove_descendants() {
        let f = Fixture::new();
        let nested = f.root.join("incoming");
        std::fs::create_dir_all(nested.join("deep")).unwrap();
        std::fs::write(nested.join("deep/report.txt"), "hello").unwrap();
        process_path(None, &f.state.inner, &nested).unwrap();
        assert_eq!(f.state.inner.db.list_folders().unwrap().len(), 1);
        assert_eq!(f.state.inner.db.list_folders().unwrap()[0].file_count, 1);
        let moved = f.root.join("moved");
        std::fs::rename(&nested, &moved).unwrap();
        process_path(None, &f.state.inner, &nested).unwrap();
        process_path(None, &f.state.inner, &moved).unwrap();
        assert_eq!(f.state.inner.db.all_files().unwrap().len(), 1);
        std::fs::remove_dir_all(&moved).unwrap();
        process_path(None, &f.state.inner, &moved).unwrap();
        f.state.inner.engine.lock().unwrap().commit().unwrap();
        assert!(f.state.inner.db.all_files().unwrap().is_empty());
        assert_eq!(
            f.state
                .inner
                .engine
                .lock()
                .unwrap()
                .search("report", 20, None)
                .unwrap()
                .total,
            0
        );
    }

    #[test]
    fn stop_does_not_mark_folder_indexed_or_disable_watcher() {
        let f = Fixture::new();
        let path = f.root.join("report.txt");
        std::fs::write(&path, "hello").unwrap();
        f.state.inner.stop_flag.store(true, Ordering::Relaxed);
        assert_eq!(f.scan(false).indexed, 0);
        assert!(f.state.inner.db.list_folders().unwrap()[0]
            .last_indexed_at
            .is_none());
        process_path(None, &f.state.inner, &f.root).unwrap();
        assert_eq!(f.state.inner.db.all_files().unwrap().len(), 1);
        assert!(f.state.inner.stop_flag.load(Ordering::Relaxed));
    }
}
