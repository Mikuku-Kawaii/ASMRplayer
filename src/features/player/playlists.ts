import { listen } from "@tauri-apps/api/event";
import { callCommand, isTauriRuntime, subscribeLibraryAffinity } from "../../lib/api";

export interface PlaylistTrack { versionId: string; title: string; workTitle: string; coverRef?: string | null; available: boolean }
export interface Playlist { id: string; name: string; versionIds: string[]; sortOrder: number; tracks: PlaylistTrack[] }
export type PlaylistTarget = { versionIds: string[] } | { workId: string } | { sourceWorkId: string };
export async function playlistCommand<T>(action: string, input: Record<string, unknown> = {}) {
  const value = await callCommand<T>("playlist_command", { action, input });
  if (["create", "update", "delete"].includes(action)) window.dispatchEvent(new Event("playlists:changed"));
  return value;
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
