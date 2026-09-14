pub mod core;
pub mod database;
pub mod scanner;
pub mod search;

use database::sqlite::Db;
use scanner::watcher::{AppState, AppStateShared};
use search::index::SearchEngine;
use std::collections::HashSet;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};
use tauri::{Manager, State};

struct Managed(Arc<AppState>);

static APP_HANDLE: OnceLock<Mutex<Option<tauri::AppHandle>>> = OnceLock::new();

/// Open the Tantivy index, retrying briefly in case a previous instance is
/// shutting down and still holds the writer lock.
fn open_search_engine(path: &Path) -> Result<SearchEngine, String> {
    const RETRIES: usize = 10;
    const DELAY: std::time::Duration = std::time::Duration::from_millis(500);
    let mut last_err = String::new();
    for attempt in 0..RETRIES {
        match SearchEngine::open(path) {
            Ok(engine) => return Ok(engine),
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("LockBusy") || msg.contains("already an `IndexWriter`") {
                    last_err = msg;
                    log::warn!("index locked, retry {}/{}…", attempt + 1, RETRIES);
                    std::thread::sleep(DELAY);
                } else {
                    return Err(msg);
                }
            }
        }
    }
    Err(last_err)
}

fn set_app_handle(h: tauri::AppHandle) {
    APP_HANDLE.get_or_init(|| Mutex::new(None)).lock().unwrap().replace(h);
}

fn app_handle() -> Option<tauri::AppHandle> {
    APP_HANDLE.get().and_then(|m| m.lock().unwrap().clone())
}

#[tauri::command]
fn list_folders(state: State<Managed>) -> Result<Vec<database::sqlite::FolderRow>, String> {
    state.0.db.list_folders().map_err(|e| e.to_string())
}

#[tauri::command]
fn add_folder(state: State<Managed>, path: String) -> Result<database::sqlite::FolderRow, String> {
    let id = state.0.db.add_folder(&path).map_err(|e| e.to_string())?;
    // index in background so the UI never blocks
    let app = app_handle().ok_or("no app")?;
    let state_bg = Arc::clone(&state.0);
    let path_bg = path.clone();
    std::thread::spawn(move || {
        if let Err(e) = scanner::watcher::index_folder(&app, &state_bg, id, &path_bg, false) {
            log::warn!("background indexing failed for {path_bg}: {e}");
            state_bg.busy.store(false, std::sync::atomic::Ordering::Relaxed);
        }
        if let Err(e) = scanner::watcher::add_watch(&state_bg, Path::new(&path_bg)) {
            log::warn!("watch registration failed for {path_bg}: {e}");
        }
    });
    let rows = state.0.db.list_folders().map_err(|e| e.to_string())?;
    rows.into_iter().find(|r| r.id == id).ok_or("folder vanished".into())
}

#[tauri::command]
fn remove_folder(state: State<Managed>, id: i64) -> Result<(), String> {
    let folders = state.0.db.list_folders().map_err(|e| e.to_string())?;
    let path = folders
        .into_iter()
        .find(|f| f.id == id)
        .map(|f| f.path)
        .ok_or("no such folder")?;
    let pairs = state.0.db.remove_folder(id).map_err(|e| e.to_string())?;
    let mut engine = state.0.engine.lock().unwrap();
    for (_, doc_id) in pairs {
        let _ = engine.remove(&doc_id);
    }
    engine.commit().map_err(|e| e.to_string())?;
    drop(engine);
    let _ = scanner::watcher::remove_watch(&state.0, Path::new(&path));
    Ok(())
}

#[tauri::command]
fn toggle_folder(state: State<Managed>, id: i64, enabled: bool) -> Result<(), String> {
    state
        .0
        .db
        .set_folder_enabled(id, enabled)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn reindex_folder(state: State<Managed>, id: i64) -> Result<(), String> {
    let app = app_handle().ok_or("no app")?;
    let folders = state.0.db.list_folders().map_err(|e| e.to_string())?;
    let folder = folders.into_iter().find(|f| f.id == id).ok_or("no such folder")?;
    let state_bg = Arc::clone(&state.0);
    std::thread::spawn(move || {
        if let Err(e) = scanner::watcher::index_folder(&app, &state_bg, id, &folder.path, false) {
            log::warn!("reindex failed for {}: {e}", folder.path);
            state_bg.busy.store(false, std::sync::atomic::Ordering::Relaxed);
        }
    });
    Ok(())
}

#[tauri::command]
fn reindex_all(state: State<Managed>) -> Result<(), String> {
    let app = app_handle().ok_or("no app")?;
    let folders = state.0.db.list_folders().map_err(|e| e.to_string())?;
    for f in folders {
        if f.enabled {
            let app_bg = app.clone();
            let state_bg = Arc::clone(&state.0);
            std::thread::spawn(move || {
                let _ = scanner::watcher::index_folder(&app_bg, &state_bg, f.id, &f.path, false);
            });
        }
    }
    Ok(())
}

#[tauri::command]
fn stop_indexing(state: State<Managed>) -> Result<(), String> {
    state.0.stop_flag.store(true, std::sync::atomic::Ordering::Relaxed);
    Ok(())
}

#[tauri::command]
fn search(query: String, state: State<Managed>) -> Result<core::SearchResponse, String> {
    let folders = state.0.db.list_folders().map_err(|e| e.to_string())?;
    let enabled: HashSet<String> = folders
        .into_iter()
        .filter(|f| f.enabled)
        .map(|f| f.path)
        .collect();
    let engine = state.0.engine.lock().unwrap();
    engine
        .search_parsed(&query, 100, &enabled)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn open_file(path: String) -> Result<(), String> {
    open_target(Path::new(&path), false)
}

#[tauri::command]
fn reveal_in_folder(path: String) -> Result<(), String> {
    open_target(Path::new(&path), true)
}

fn open_target(path: &Path, reveal: bool) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let mut cmd = std::process::Command::new("open");
        if reveal {
            cmd.args(["-R"]).arg(path);
        } else {
            cmd.arg(path);
        }
        cmd.spawn().map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "windows")]
    {
        if reveal {
            std::process::Command::new("explorer")
                .arg(format!("/select,{}", path.display()))
                .spawn()
                .map_err(|e| e.to_string())?;
        } else {
            std::process::Command::new("cmd")
                .args(["/C", "start", "", &path.to_string_lossy()])
                .spawn()
                .map_err(|e| e.to_string())?;
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        std::process::Command::new("xdg-open").arg(path).spawn().map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn folder_stats(state: State<Managed>) -> Result<serde_json::Value, String> {
    let folders = state.0.db.list_folders().map_err(|e| e.to_string())?;
    let total: i64 = folders.iter().map(|f| f.file_count).sum();
    Ok(serde_json::json!({ "folders": folders.len(), "files": total }))
}

#[tauri::command]
fn recent_errors(state: State<Managed>) -> Result<Vec<(String, String, i64)>, String> {
    state.0.db.recent_errors(50).map_err(|e| e.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(
            tauri_plugin_log::Builder::new()
                .target(tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::Stdout))
                .build(),
        )
        .setup(|app| {
            set_app_handle(app.handle().clone());

            let db = Db::open(&Db::default_db_path())
                .map_err(|e| format!("SQLite 초기화 실패: {e}"))?;
            let engine = open_search_engine(&Db::default_index_path())
                .map_err(|e| format!("인덱스 초기화 실패: {e}\n(이미 실행 중인 FileSearcher가 있으면 종료한 뒤 다시 시도하세요)"))?;
            let shared = AppStateShared::new(db, engine);
            app.manage(Managed(Arc::clone(&shared.inner)));

            // startup reconciliation + watcher in background thread
            let handle = app.handle().clone();
            let state = Arc::clone(&shared.inner);
            std::thread::spawn(move || {
                let _ = scanner::watcher::reconcile_on_startup(&handle, &state);
                let roots = scanner::watcher::all_watched_roots(&state);
                if let Err(e) = scanner::watcher::start_watcher(handle, &state, roots) {
                    log::warn!("watcher start failed: {e}");
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_folders,
            add_folder,
            remove_folder,
            toggle_folder,
            reindex_folder,
            reindex_all,
            stop_indexing,
            search,
            open_file,
            reveal_in_folder,
            folder_stats,
            recent_errors
        ])
        .run(tauri::generate_context!())
        .inspect_err(|e| {
            eprintln!("앱 실행 실패: {e}");
        })
        .ok();
}
