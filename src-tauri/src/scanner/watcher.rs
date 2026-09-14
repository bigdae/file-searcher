use crate::database::sqlite::Db;
use crate::search::index::SearchEngine;
use anyhow::Result;
use notify::Watcher;
use notify_debouncer_full::{DebouncedEvent, Debouncer, FileIdMap, new_debouncer};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::Emitter;

pub struct AppState {
    pub db: Db,
    pub engine: Mutex<SearchEngine>,
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
                busy: AtomicBool::new(false),
                stop_flag: AtomicBool::new(false),
                watcher: Mutex::new(None),
            }),
        }
    }
}

pub fn emit_status(app: &tauri::AppHandle, state: &AppState, message: &str, progress: Option<(usize, usize)>) {
    let _ = app.emit("index-status", serde_json::json!({
        "busy": state.busy.load(Ordering::Relaxed),
        "message": message,
        "done": progress.map(|p| p.0),
        "total": progress.map(|p| p.1),
    }));
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
    let excludes: HashSet<String> = HashSet::new();
    let files = crate::scanner::scanner::scan(Path::new(folder_path), &excludes);

    let total = files.len();
    let mut indexed = 0usize;
    let mut skipped = 0usize;
    let mut removed = 0usize;

    state.stop_flag.store(false, Ordering::Relaxed);

    // collect DB paths under this folder to detect deletions
    let mut known: HashMap<String, ()> = HashMap::new();
    {
        for (p, _) in state.db.all_files()? {
            if crate::core::path_under(&p, folder_path) {
                known.insert(p, ());
            }
        }
    }

    state.busy.store(true, Ordering::Relaxed);

    emit_status(app, state, &format!("스캔 완료: {total}개 파일"), Some((0, total)));

    for (i, f) in files.iter().enumerate() {
        if state.stop_flag.load(Ordering::Relaxed) {
            break;
        }
        known.remove(&f.path);

        if incremental {
            if let Some(meta) = state.db.get_file(&f.path)? {
                if meta.size == f.size && meta.mtime == f.mtime && meta.status == "ok" {
                    skipped += 1;
                    continue;
                }
            }
        }

        let doc_id = uuid::Uuid::new_v4().to_string();
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
            engine.commit()?;
        }
        state.db.upsert_file(folder_id, &f.path, f.size, f.mtime, &doc_id, "ok")?;
        indexed += 1;

        if i % 25 == 0 {
            emit_status(app, state, &format!("인덱싱 중… {}/{}", i + 1, total), Some((i + 1, total)));
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

    state.db.touch_folder(folder_id)?;
    emit_status(app, state, "인덱싱 완료", Some((total, total)));
    state.busy.store(false, Ordering::Relaxed);
    Ok(IndexOutcome { indexed, skipped, removed })
}

/// Start watching a folder with debounce; events trigger incremental re-index
/// of the affected files.
pub fn start_watcher(app: tauri::AppHandle, state: &Arc<AppState>, folders: Vec<PathBuf>) -> Result<()> {
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
        debouncer
            .watcher()
            .watch(folder, notify::RecursiveMode::Recursive)?;
    }

    *state.watcher.lock().unwrap() = Some(debouncer);
    Ok(())
}

#[allow(dead_code)]
pub fn stop_watcher(state: &AppState) {
    *state.watcher.lock().unwrap() = None;
}

/// Watch a folder added at runtime (debouncer is shared with the startup one).
pub fn add_watch(state: &AppState, folder: &Path) -> Result<()> {
    if let Some(deb) = state.watcher.lock().unwrap().as_mut() {
        deb.watcher().watch(folder, notify::RecursiveMode::Recursive)?;
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
    if state.busy.load(Ordering::Relaxed) {
        return;
    }
    state.busy.store(true, Ordering::Relaxed);

    let mut seen: HashSet<String> = HashSet::new();
    for ev in &events {
        for path in &ev.paths {
            let p = path.to_string_lossy().to_string();
            if seen.insert(p.clone()) {
                let _ = process_path(app, state, path);
            }
        }
    }

    state.busy.store(false, Ordering::Relaxed);
    emit_status(app, state, "변경 반영 완료", None);
}

fn process_path(app: &tauri::AppHandle, state: &AppState, path: &Path) -> Result<()> {
    let path_str = path.to_string_lossy().to_string();

    if !path.exists() {
        if let Some(doc_id) = state.db.delete_file(&path_str)? {
            state.engine.lock().unwrap().remove(&doc_id)?;
            state.engine.lock().unwrap().commit()?;
        }
        return Ok(());
    }

    if path.is_dir() {
        // folder moved in: index it
        let folder_id = state.db.add_folder(&path_str)?;
        index_folder(app, state, folder_id, &path_str, false)?;
        return Ok(());
    }

    let ext = crate::core::extension_of(&path_str);

    let meta = std::fs::metadata(path)?;
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    // find owning folder
    let folders = state.db.list_folders()?;
    let owner = folders
        .iter()
        .find(|f| crate::core::path_under(&path_str, &f.path) && f.enabled)
        .map(|f| (f.id, f.path.clone()));

    let Some(folder_id) = owner.map(|(id, _)| id) else {
        return Ok(());
    };

    // existing doc?
    if let Some(existing) = state.db.get_file(&path_str)? {
        if existing.size == meta.len() && existing.mtime == mtime {
            return Ok(());
        }
        // remove old doc
        if let Some(doc_id) = state.db.doc_id_of(&path_str)? {
            state.engine.lock().unwrap().remove(&doc_id)?;
        }
    }

    let doc_id = uuid::Uuid::new_v4().to_string();
    state.engine.lock().unwrap().add_or_replace(
        &doc_id,
        &path_str,
        &crate::core::filename_of(&path_str),
        &ext,
        mtime,
        meta.len(),
    )?;
    state.engine.lock().unwrap().commit()?;
    state.db.upsert_file(folder_id, &path_str, meta.len(), mtime, &doc_id, "ok")?;

    emit_status(app, state, &format!("갱신: {}", crate::core::filename_of(&path_str)), None);
    Ok(())
}

/// Startup reconciliation: re-check metadata of known files and pick up
/// changes that happened while the app was closed.
pub fn reconcile_on_startup(app: &tauri::AppHandle, state: &AppState) -> Result<IndexOutcome> {
    let folders = state.db.list_folders()?;
    let mut total = IndexOutcome { indexed: 0, skipped: 0, removed: 0 };

    for folder in &folders {
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
                log::warn!("reconcile failed for {}: {e}", folder.path);
            }
        }
    }
    emit_status(app, state, "시작 정합성 확인 완료", None);
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
