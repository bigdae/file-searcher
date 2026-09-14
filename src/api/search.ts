import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open as openDialog } from "@tauri-apps/plugin-dialog";

export interface FolderRow {
  id: number;
  path: string;
  enabled: boolean;
  file_count: number;
  last_indexed_at: number | null;
}

export interface SearchHit {
  doc_id: string;
  path: string;
  filename: string;
  extension: string;
  size: number;
  modified_at: number;
  score: number;
  snippet: string;
}

export interface SearchResponse {
  hits: SearchHit[];
  total: number;
  elapsed_ms: number;
}

export interface IndexStatus {
  busy: boolean;
  message: string;
  done: number | null;
  total: number | null;
}

export async function listFolders(): Promise<FolderRow[]> {
  return invoke<FolderRow[]>("list_folders");
}

export async function addFolder(): Promise<string | null> {
  const picked = await openDialog({ directory: true, multiple: false });
  if (!picked) return null;
  await invoke("add_folder", { path: picked });
  return picked;
}

export async function removeFolder(id: number): Promise<void> {
  await invoke("remove_folder", { id });
}

export async function toggleFolder(id: number, enabled: boolean): Promise<void> {
  await invoke("toggle_folder", { id, enabled });
}

export async function reindexFolder(id: number): Promise<void> {
  await invoke("reindex_folder", { id });
}

export async function reindexAll(): Promise<void> {
  await invoke("reindex_all");
}

export async function stopIndexing(): Promise<void> {
  await invoke("stop_indexing");
}

export type SearchScope = "filename" | "filename_content";

export async function search(query: string, scope: SearchScope): Promise<SearchResponse> {
  return invoke<SearchResponse>("search", { query, scope });
}

export async function openFile(path: string): Promise<void> {
  await invoke("open_file", { path });
}

export async function revealInFolder(path: string): Promise<void> {
  await invoke("reveal_in_folder", { path });
}

export async function recentErrors(): Promise<[string, string, number][]> {
  return invoke<[string, string, number][]>("recent_errors");
}

export function onIndexStatus(cb: (s: IndexStatus) => void): Promise<() => void> {
  return listen<IndexStatus>("index-status", (e) => cb(e.payload));
}
