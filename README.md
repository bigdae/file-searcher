# FileSearcher

Local file search for Windows and macOS. Register the folders you want to search and FileSearcher builds an index up front, then answers filename and path queries instantly. File changes are watched and reflected in the index incrementally.

Built with Tauri 2 + React/TypeScript + Rust + Tantivy + SQLite.

## Features

- **Folder management** — add/remove search folders, enable/disable, reindex one folder or all, stop indexing
- **Fast search** — Tantivy full-text index with relevance, modified date, filename, and size sorting
- **Partial matching** — a 1–3 character ngram tokenizer on filenames so substrings match (e.g. `회의` finds `회의록_2026.docx`)
- **Query operators** — `name:`, `path:`, `ext:`, and `"quoted phrases"`
- **Incremental updates** — create/modify/rename/delete events from `notify` are applied to the index, and a startup reconciliation pass catches changes missed while the app was closed
- **Result UX** — file type icons, query highlighting, click to open, click the path to reveal in Explorer/Finder
- **Recent files panel** — last 100 opened files, stored locally
- **Indexing status bar** — live progress shown in the top bar
- **Default excludes** — `node_modules`, `.git`, `target`, `dist`, `build`, `.cache`, `~$*`, `.DS_Store`

> Version 0.2.4 searches **filenames and paths**. Document content search (Office/PDF) is planned in [cross-platform-file-search-plan.md](cross-platform-file-search-plan.md).

## Query syntax

| Input | Description |
|---|---|
| `report` | Search filenames (multiple terms are ANDed) |
| `"smart factory plan"` | Exact phrase search |
| `name:meeting` | Restrict to the filename field |
| `path:projects` | Path filter |
| `ext:pdf` | Extension filter |
| `report ext:pptx` | Combined conditions |

## Tech stack

| Area | Technology |
|---|---|
| Desktop app | Tauri 2 |
| UI | React 18 + TypeScript + Vite |
| Core | Rust |
| Search index | Tantivy 0.22 |
| State DB | SQLite (rusqlite, bundled) |
| Scan/watch | walkdir, notify, notify-debouncer-full |
| Async | Tokio |

## Requirements

- Node.js 20+
- Rust stable (rust-version 1.77.2 or newer)
- Platform-specific Tauri prerequisites
  - Windows: MSVC build tools, WebView2 (included on Windows 11)
  - macOS: Xcode Command Line Tools

## Development

```bash
npm install
npx tauri dev        # Vite dev server + Tauri window
```

Frontend-only build:

```bash
npm run build        # tsc && vite build
```

Makefile targets are also available: `make dev`, `make build`, `make test`, `make check`, `make fmt`, `make clippy`, `make frontend`, `make clean`.

## Building installers

```bash
npx tauri build
```

Artifacts:

| Platform | Path |
|---|---|
| Windows (NSIS) | `src-tauri/target/release/bundle/nsis/FileSearcher_<version>_x64-setup.exe` |
| Windows (MSI) | `src-tauri/target/release/bundle/msi/FileSearcher_<version>_x64_en-US.msi` |
| macOS | `src-tauri/target/release/bundle/dmg/FileSearcher_<version>_*.dmg` |

## Tests

```bash
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml
```

## Project structure

```text
src/                          # React UI
├─ App.tsx                    # Search/Settings tabs + indexing status bar
├─ pages/
│  ├─ SearchPage.tsx          # Search, results, recent files panel
│  └─ SettingsPage.tsx        # Search folder management
└─ api/search.ts              # Tauri IPC wrappers

src-tauri/src/
├─ lib.rs                     # Tauri commands and app setup
├─ core.rs                    # Shared types and path helpers
├─ search/
│  ├─ index.rs                # Tantivy schema and search engine
│  └─ query.rs                # Query parser (name:/path:/ext:)
├─ scanner/
│  ├─ scanner.rs              # Directory scan and exclude rules
│  └─ watcher.rs              # File watching, incremental indexing, startup reconciliation
└─ database/
   └─ sqlite.rs               # Folder/file metadata and error log
```

## Data locations

| Item | Windows | macOS |
|---|---|---|
| Folder | `%APPDATA%\file-searcher\` | `~/Library/Application Support/file-searcher/` |

- `index.db` — SQLite (folders, file metadata, indexing errors)
- `tantivy/` — Tantivy search index

On startup the app prunes documents missing from SQLite, compares file metadata (path, size, mtime), and reindexes whatever changed.

## Roadmap

See [cross-platform-file-search-plan.md](cross-platform-file-search-plan.md) for the full plan.

- Document content extraction and content search (`.docx`, `.pptx`, `.xlsx`, `.pdf`, etc.)
- Result snippets showing match context
- Image OCR, natural-language/hybrid search (later)
- Auto-update, code signing and notarization
