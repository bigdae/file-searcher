import { useEffect, useState } from "react";
import SearchPage from "./pages/SearchPage";
import SettingsPage from "./pages/SettingsPage";
import { onIndexStatus, IndexStatus } from "./api/search";

type Tab = "search" | "settings";

export default function App() {
  const [tab, setTab] = useState<Tab>("search");
  const [indexVersion, setIndexVersion] = useState(0);
  const [status, setStatus] = useState<IndexStatus>({ busy: false, message: "", done: null, total: null });

  useEffect(() => {
    let disposed = false;
    let un: (() => void) | undefined;
    onIndexStatus((s) => {
      if (disposed) return;
      setStatus(s);
      if (!s.busy) setIndexVersion((version) => version + 1);
    })
      .then((f) => {
        if (disposed) f();
        else un = f;
      })
      .catch((error) => {
        if (!disposed) setStatus({ busy: false, message: `상태 수신 실패: ${error}`, done: null, total: null });
      });
    return () => {
      disposed = true;
      un?.();
    };
  }, []);

  return (
    <div className="app">
      <header className="topbar">
        <div className="tabs">
          <button className={tab === "search" ? "tab active" : "tab"} onClick={() => setTab("search")}>
            검색
          </button>
          <button className={tab === "settings" ? "tab active" : "tab"} onClick={() => setTab("settings")}>
            설정
          </button>
        </div>
        <div className="status">
          {status.busy && <span className="spinner" />}
          <span className="status-text">
            {status.busy
              ? `${status.message}${status.done != null && status.total ? ` (${status.done}/${status.total})` : ""}`
              : status.message || "준비"}
          </span>
        </div>
      </header>
      <main className="content">{tab === "search" ? <SearchPage indexVersion={indexVersion} /> : <SettingsPage />}</main>
    </div>
  );
}
