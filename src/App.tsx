import { useEffect, useState } from "react";
import SearchPage from "./pages/SearchPage";
import SettingsPage from "./pages/SettingsPage";
import { onIndexStatus, IndexStatus } from "./api/search";

type Tab = "search" | "settings";

export default function App() {
  const [tab, setTab] = useState<Tab>("search");
  const [status, setStatus] = useState<IndexStatus>({ busy: false, message: "", done: null, total: null });

  useEffect(() => {
    let un: (() => void) | undefined;
    onIndexStatus((s) => setStatus(s)).then((f) => (un = f));
    return () => un?.();
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
      <main className="content">{tab === "search" ? <SearchPage /> : <SettingsPage />}</main>
    </div>
  );
}
