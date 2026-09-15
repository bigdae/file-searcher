import React, { useEffect, useState } from "react";
import {
  search,
  openFile,
  revealInFolder,
  SearchHit,
  SearchResponse,
} from "../api/search";

interface RecentFile {
  path: string;
  filename: string;
  extension: string;
  opened_at: number;
}

const RECENT_KEY = "file-searcher.recent";
const RECENT_LIMIT = 100;

function loadRecent(): RecentFile[] {
  try {
    const raw = localStorage.getItem(RECENT_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw);
    return Array.isArray(parsed) ? (parsed as RecentFile[]) : [];
  } catch {
    return [];
  }
}

const FILE_TYPE_GROUPS: { label: string; color: string; exts: string[] }[] = [
  { label: "PDF", color: "#dc2626", exts: ["pdf"] },
  { label: "DOC", color: "#2563eb", exts: ["doc", "docx", "rtf", "odt", "hwp", "hwpx"] },
  { label: "XLS", color: "#16a34a", exts: ["xls", "xlsx", "csv", "ods"] },
  { label: "PPT", color: "#ea580c", exts: ["ppt", "pptx", "odp", "key"] },
  {
    label: "IMG",
    color: "#9333ea",
    exts: ["png", "jpg", "jpeg", "gif", "bmp", "svg", "webp", "ico", "tif", "tiff", "heic"],
  },
  {
    label: "VID",
    color: "#db2777",
    exts: ["mp4", "mkv", "avi", "mov", "wmv", "flv", "webm", "m4v"],
  },
  { label: "AUD", color: "#0891b2", exts: ["mp3", "wav", "flac", "aac", "ogg", "m4a", "wma"] },
  { label: "ZIP", color: "#78716c", exts: ["zip", "rar", "7z", "tar", "gz", "bz2"] },
  { label: "TXT", color: "#64748b", exts: ["txt", "md", "log", "ini", "cfg", "conf"] },
  {
    label: "CODE",
    color: "#0d9488",
    exts: [
      "js", "ts", "jsx", "tsx", "py", "java", "c", "cpp", "h", "cs", "go", "rs", "rb", "php",
      "html", "css", "json", "xml", "yml", "yaml", "sh", "bat", "ps1", "sql", "vue", "svelte",
    ],
  },
  { label: "EXE", color: "#4f46e5", exts: ["exe", "msi", "dll", "bin"] },
];

function fileIcon(ext: string): { label: string; color: string } {
  const e = ext.toLowerCase().replace(/^\./, "");
  if (!e) return { label: "FILE", color: "#64748b" };
  for (const g of FILE_TYPE_GROUPS) {
    if (g.exts.includes(e)) return { label: g.label, color: g.color };
  }
  return { label: e.slice(0, 4).toUpperCase(), color: "#64748b" };
}

function FileIcon({ extension }: { extension: string }) {
  const { label, color } = fileIcon(extension);
  return (
    <span className="file-icon" style={{ backgroundColor: color }}>
      {label}
    </span>
  );
}

function highlight(text: string, query: string): React.ReactNode {
  const terms = query
    .split(/[\s"]+/)
    .filter((t) => t && !t.includes(":"));
  if (terms.length === 0) return text;
  const pattern = terms
    .map((t) => t.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"))
    .join("|");
  const re = new RegExp(`(${pattern})`, "gi");
  return text.split(re).map((part, i) =>
    terms.some((t) => t.toLowerCase() === part.toLowerCase()) ? (
      <mark key={i}>{part}</mark>
    ) : (
      part
    ),
  );
}

export default function SearchPage({ indexVersion }: { indexVersion: number }) {
  const [query, setQuery] = useState("");
  const [result, setResult] = useState<SearchResponse | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [sortKey, setSortKey] = useState<"score" | "modified" | "name" | "size">("score");
  const [recent, setRecent] = useState<RecentFile[]>(loadRecent);

  useEffect(() => {
    try {
      localStorage.setItem(RECENT_KEY, JSON.stringify(recent));
    } catch {}
  }, [recent]);

  useEffect(() => {
    let active = true;
    setResult(null);
    setError(null);
    setLoading(false);
    if (!query.trim()) return;

    const timer = window.setTimeout(async () => {
      if (!active) return;
      setLoading(true);
      try {
        const res = await search(query);
        if (active) setResult(res);
      } catch (e) {
        if (active) setError(String(e));
      } finally {
        if (active) setLoading(false);
      }
    }, 200);
    return () => {
      active = false;
      window.clearTimeout(timer);
    };
  }, [query, indexVersion]);

  const hits = [...(result?.hits ?? [])].sort((a, b) => {
    switch (sortKey) {
      case "modified":
        return b.modified_at - a.modified_at;
      case "name":
        return a.filename.localeCompare(b.filename);
      case "size":
        return b.size - a.size;
      default:
        return b.score - a.score;
    }
  });

  async function handleFileAction(action: () => Promise<void>) {
    try {
      await action();
    } catch (e) {
      setError(String(e));
    }
  }

  function pushRecent(file: { path: string; filename: string; extension: string }) {
    setRecent((prev) =>
      [
        { path: file.path, filename: file.filename, extension: file.extension, opened_at: Date.now() },
        ...prev.filter((r) => r.path !== file.path),
      ].slice(0, RECENT_LIMIT),
    );
  }

  async function openHit(file: { path: string; filename: string; extension: string }) {
    pushRecent(file);
    await handleFileAction(() => openFile(file.path));
  }

  return (
    <div className="search-page">
      <div className="search-main">
        <div className="search-controls">
          <input
            className="search-box"
            autoFocus
            placeholder="파일명 검색 (예: 보고서, name:회의록, ext:pdf)"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
          />
        </div>

        <div className="result-meta">
          {result && (
            <>
              <span>
                {result.total.toLocaleString()}개 결과 · {result.elapsed_ms}ms
              </span>
              <select
                value={sortKey}
                onChange={(e) => setSortKey(e.target.value as typeof sortKey)}
                className="sort-select"
              >
                <option value="score">관련도순</option>
                <option value="modified">최신 수정순</option>
                <option value="name">파일명순</option>
                <option value="size">파일 크기순</option>
              </select>
            </>
          )}
          {loading && <span className="muted">검색 중…</span>}
          {error && <span className="error">{error}</span>}
        </div>

        <div className="results">
          {hits.map((hit: SearchHit) => (
            <div className="hit" key={hit.doc_id}>
              <div className="hit-row">
                <div className="hit-title" onClick={() => openHit(hit)}>
                  <FileIcon extension={hit.extension} />
                  <span className="hit-name">{highlight(hit.filename, query)}</span>
                </div>
                <div
                  className="hit-path"
                  title={hit.path}
                  onClick={() => handleFileAction(() => revealInFolder(hit.path))}
                >
                  {hit.path}
                </div>
              </div>
            </div>
          ))}
          {result && hits.length === 0 && !loading && (
            <div className="empty">검색 결과가 없습니다.</div>
          )}
        </div>
      </div>

      <aside className="recent-panel">
        <div className="recent-head">
          <h2>최근 파일</h2>
          {recent.length > 0 && (
            <button className="recent-clear" onClick={() => setRecent([])}>
              전체 지우기
            </button>
          )}
        </div>
        <ul className="recent-list">
          {recent.map((r) => (
            <li key={r.path} className="recent-item" title={r.path} onClick={() => openHit(r)}>
              <FileIcon extension={r.extension} />
              <div className="recent-info">
                <div className="recent-name">{r.filename}</div>
              </div>
              <button
                className="recent-remove"
                title="목록에서 제거"
                onClick={(e) => {
                  e.stopPropagation();
                  setRecent((prev) => prev.filter((item) => item.path !== r.path));
                }}
              >
                ×
              </button>
            </li>
          ))}
          {recent.length === 0 && <li className="empty">최근 연 파일이 없습니다.</li>}
        </ul>
      </aside>
    </div>
  );
}
