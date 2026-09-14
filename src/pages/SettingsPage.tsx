import { useEffect, useState } from "react";
import {
  listFolders,
  addFolder,
  removeFolder,
  toggleFolder,
  reindexFolder,
  reindexAll,
  stopIndexing,
  recentErrors,
  FolderRow,
} from "../api/search";

export default function SettingsPage() {
  const [folders, setFolders] = useState<FolderRow[]>([]);
  const [errors, setErrors] = useState<[string, string, number][]>([]);
  const [busyFolder, setBusyFolder] = useState<number | null>(null);
  const [addError, setAddError] = useState<string | null>(null);
  const [stopping, setStopping] = useState(false);
  const [stopError, setStopError] = useState<string | null>(null);

  async function refresh() {
    try {
      const [nextFolders, nextErrors] = await Promise.all([listFolders(), recentErrors()]);
      setFolders(nextFolders);
      setErrors(nextErrors);
    } catch (e) {
      setAddError(`목록 조회 실패: ${e}`);
    }
  }

  useEffect(() => {
    refresh();
    const t = setInterval(refresh, 3000);
    return () => clearInterval(t);
  }, []);

  async function handleAdd() {
    setBusyFolder(-1);
    setAddError(null);
    try {
      await addFolder();
      await refresh();
    } catch (e) {
      setAddError(`폴더 추가 실패: ${e}`);
    } finally {
      setBusyFolder(null);
    }
  }

  async function handleRemove(id: number) {
    await handleAction(id, "폴더 제거", () => removeFolder(id));
  }

  async function handleToggle(f: FolderRow) {
    await handleAction(f.id, "폴더 설정 변경", () => toggleFolder(f.id, !f.enabled));
  }

  async function handleReindex(id: number) {
    await handleAction(id, "재인덱싱", () => reindexFolder(id));
  }

  async function handleStop() {
    setStopping(true);
    setStopError(null);
    try {
      await stopIndexing();
    } catch (e) {
      setStopError(`인덱싱 중지 실패: ${e}`);
    } finally {
      setStopping(false);
    }
  }

  async function handleAction(id: number, label: string, action: () => Promise<void>) {
    setBusyFolder(id);
    setAddError(null);
    try {
      await action();
      await refresh();
    } catch (e) {
      setAddError(`${label} 실패: ${e}`);
    } finally {
      setBusyFolder(null);
    }
  }

  function formatTime(ts: number | null): string {
    if (!ts) return "아직";
    return new Date(ts * 1000).toLocaleString("ko-KR");
  }

  return (
    <div className="settings-page">
      <section className="panel">
        <div className="panel-head">
          <h2>검색 위치</h2>
          <div className="panel-actions">
            <button onClick={handleAdd} disabled={busyFolder !== null}>
              + 폴더 추가
            </button>
            <button onClick={() => handleAction(-2, "전체 재인덱싱", reindexAll)} disabled={busyFolder !== null}>전체 재인덱싱</button>
            <button onClick={handleStop} disabled={stopping}>인덱싱 중지</button>
          </div>
        </div>
        {addError && <div className="error-msg" style={{ marginBottom: 8 }}>{addError}</div>}
        {stopError && <div className="error-msg" style={{ marginBottom: 8 }}>{stopError}</div>}
        <ul className="folder-list">
          {folders.map((f) => (
            <li key={f.id} className={f.enabled ? "" : "disabled"}>
              <label className="switch">
                <input type="checkbox" checked={f.enabled} disabled={busyFolder !== null} onChange={() => handleToggle(f)} />
                <span />
              </label>
              <div className="folder-info">
                <div className="folder-path">{f.path}</div>
                <div className="folder-sub">
                  {f.file_count.toLocaleString()} files · 마지막 인덱싱: {formatTime(f.last_indexed_at)}
                </div>
              </div>
              <div className="folder-actions">
                <button onClick={() => handleReindex(f.id)} disabled={busyFolder !== null || !f.enabled}>
                  {busyFolder === f.id ? "…" : "재인덱싱"}
                </button>
                <button className="danger" onClick={() => handleRemove(f.id)} disabled={busyFolder !== null}>
                  제거
                </button>
              </div>
            </li>
          ))}
          {folders.length === 0 && (
            <li className="empty">등록된 폴더가 없습니다. + 폴더 추가를 눌러 검색 위치를 등록하세요.</li>
          )}
        </ul>
      </section>

      <section className="panel">
        <h2>오류 파일 (최근 50건)</h2>
        <ul className="error-list">
          {errors.map((e, i) => (
            <li key={i}>
              <div className="error-path">{e[0]}</div>
              <div className="error-msg">{e[1]}</div>
            </li>
          ))}
          {errors.length === 0 && <li className="empty">오류가 없습니다.</li>}
        </ul>
      </section>
    </div>
  );
}
