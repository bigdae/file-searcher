import React, { useCallback, useEffect, useRef, useState } from "react";
import {
  search,
  openFile,
  revealInFolder,
  SearchHit,
  SearchResponse,
} from "../api/search";

function formatSize(bytes: number): string {
  if (bytes >= 1024 * 1024 * 1024) return `${(bytes / 1024 / 1024 / 1024).toFixed(1)} GB`;
  if (bytes >= 1024 * 1024) return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
  if (bytes >= 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${bytes} B`;
}

function formatDate(ts: number): string {
  if (!ts) return "";
  return new Date(ts * 1000).toLocaleDateString("ko-KR", {
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
  });
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

export default function SearchPage() {
  const [query, setQuery] = useState("");
  const [result, setResult] = useState<SearchResponse | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [sortKey, setSortKey] = useState<"score" | "modified" | "name" | "size">("score");
  const debounceRef = useRef<number | null>(null);

  const runSearch = useCallback(
    async (q: string) => {
      if (!q.trim()) {
        setResult(null);
        return;
      }
      setLoading(true);
      setError(null);
      try {
        const res = await search(q);
        setResult(res);
      } catch (e) {
        setError(String(e));
      } finally {
        setLoading(false);
      }
    },
    [],
  );

  useEffect(() => {
    if (debounceRef.current) window.clearTimeout(debounceRef.current);
    if (!query.trim()) {
      setResult(null);
      return;
    }
    debounceRef.current = window.setTimeout(() => runSearch(query), 200);
    return () => {
      if (debounceRef.current) window.clearTimeout(debounceRef.current);
    };
  }, [query, runSearch]);

  const hits = result?.hits ?? [...(result?.hits ?? [])].sort((a, b) => {
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

  return (
    <div className="search-page">
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
            <div className="hit-title" onClick={() => openFile(hit.path)}>
              {highlight(hit.filename, query)}
            </div>
            <div className="hit-path" onClick={() => revealInFolder(hit.path)}>
              {hit.path}
            </div>
            <div className="hit-info">
              {hit.extension ? `${hit.extension.toUpperCase()} · ` : ""}
              {formatSize(hit.size)} · {formatDate(hit.modified_at)}
            </div>
          </div>
        ))}
        {result && hits.length === 0 && !loading && (
          <div className="empty">검색 결과가 없습니다.</div>
        )}
      </div>
    </div>
  );
}
