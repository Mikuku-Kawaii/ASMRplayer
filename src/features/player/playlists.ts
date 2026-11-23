import { listen } from "@tauri-apps/api/event";
import { callCommand, isTauriRuntime, subscribeLibraryAffinity } from "../../lib/api";

export interface CollectionWork { id: string; directory?: string; format?: string; fromSite?: boolean }
export interface CollectionCard { id: string; title: string; openable?: boolean; coverRef?: string | null; available: boolean; lastPositionMs?: number | null; completed?: boolean }
export interface PlaylistTrack { versionId: string; title: string; workTitle: string; workId?: string | null; trackId?: string | null; workTrackCount?: number; versionLabel?: string | null; durationMs?: number | null; coverRef?: string | null; available: boolean }
export interface Playlist { id: string; name: string; versionIds: string[]; sortOrder: number; tracks: PlaylistTrack[]; workItems?: CollectionWork[] | null; works?: CollectionCard[] }
export type PlaylistTarget = { versionIds: string[] } | ({ workId: string } | { sourceWorkId: string }) & { directory?: string; format?: string };
export async function playlistCommand<T>(action: string, input: Record<string, unknown> = {}) {
  const value = await callCommand<T>("playlist_command", { action, input });
  if (["create", "update", "changeMembers", "delete"].includes(action)) window.dispatchEvent(new Event("playlists:changed"));
  return value;
}
const recentKey = "asmr.playlists.recent";
export function recentPlaylists(): string[] {
  try { const ids: unknown = JSON.parse(localStorage.getItem(recentKey) || "[]"); return Array.isArray(ids) ? ids.filter((v): v is string => typeof v === "string") : []; } catch { return []; }
}
export function markPlaylistUsed(id: string) {
  try { localStorage.setItem(recentKey,JSON.stringify([id,...recentPlaylists().filter(v=>v!==id)].slice(0,50))); } catch { /* Persistence is optional. */ }
}
export function requestPlaylist(target: PlaylistTarget) { window.dispatchEvent(new CustomEvent("playlists:add", { detail: target })); }
export function watchPlaylists(reload: () => void) {
  let disposed = false;
  const stops: (() => void)[] = [];
  const register = (promise: Promise<() => void>) => void promise.then(stop => { if (disposed) stop(); else stops.push(stop); }).catch(() => {});
  window.addEventListener("playlists:changed", reload);
  register(subscribeLibraryAffinity(reload));
  if (isTauriRuntime()) { register(listen("playlists:updated", reload)); register(listen("settings:updated", reload)); }
  return () => { disposed = true; stops.forEach(stop => stop()); window.removeEventListener("playlists:changed", reload); };
}
