import { useEffect, useRef, useState } from "react";
import { Heart, HeartPulse, LibraryBig } from "lucide-react";
import { AppError, callCommand, MediaAffinity, normalizeAppError, subscribeLibraryAffinity } from "../../lib/api";

export function PlayerLibraryActions({ versionId, onError, variant = "icons" }: { versionId?: string | null; onError: (e: AppError) => void; variant?: "icons" | "island-menu" }) {
  const [affinity, setAffinity] = useState<MediaAffinity | null>(null);
  const [busy, setBusy] = useState(false);
  const gate = useRef(false);
  const request = useRef(0);
  const currentVersion = useRef(versionId);
  currentVersion.current = versionId;
  useEffect(() => {
    let disposed = false;
    let stop: (() => void) | undefined;
    const refresh = async () => {
      const id = ++request.current;
      if (!versionId) { setAffinity(null); return; }
      try { const next = await callCommand<MediaAffinity>("get_media_affinity", { versionId }); if (!disposed && id === request.current) setAffinity(next); }
      catch (e) { if (!disposed && id === request.current) onError(normalizeAppError(e)); }
    };
    void refresh();
    void subscribeLibraryAffinity(() => { void refresh(); }).then(fn => { if (disposed) fn(); else stop = fn; });
    return () => { disposed = true; stop?.(); };
  }, [versionId, onError]);
  const value = affinity?.versionId === versionId ? affinity : null;
  async function run(command: string) {
    if (!value || gate.current) return;
    gate.current = true; setBusy(true);
    try {
      await callCommand(command, command === "toggle_version_favorite" ? { versionId } : { workId: value.workId });
      if (currentVersion.current !== versionId) return;
      const id = ++request.current;
      const next = await callCommand<MediaAffinity>("get_media_affinity", { versionId });
      if (id === request.current) setAffinity(next);
    } catch (e) { onError(normalizeAppError(e)); }
    finally { gate.current = false; setBusy(false); }
  }
  const actions = [
    ...(!value?.isLocalImported ? [{ key: "toggle_collected", label: value?.isCollected ? "移出音频库" : "移入我的音频库", active: value?.isCollected, disabled: false, Icon: LibraryBig }] : []),
    { key: "toggle_favorite", label: value?.isFavorite ? "取消喜爱作品" : "喜爱作品", active: value?.isFavorite, disabled: false, Icon: Heart },
    { key: "toggle_version_favorite", label: value?.isVersionFavorite ? "取消喜爱单曲" : "喜爱单曲", active: value?.isVersionFavorite, disabled: false, Icon: HeartPulse },
  ];
  const menu = variant === "island-menu";
  const menuLabels: Record<string, string> = { toggle_collected: "移入音频库", toggle_favorite: "添加喜爱", toggle_version_favorite: "添加单曲喜爱" };
  return <div className={menu ? "island-menu-column" : "player-library-actions"} role="group" aria-label="音频库与喜爱">{actions.map(({ key, label, active, disabled, Icon }) => {
    const text = menu && !active ? menuLabels[key] ?? label : label;
    return <button key={key} type="button" className={`${menu ? "island-menu-item" : "btn btn-icon"} ${key !== "toggle_collected" ? "favorite-action" : ""} ${active ? "active" : ""}`} title={text} aria-label={text} aria-pressed={!!active} disabled={!value || busy || disabled} onClick={() => void run(key)}><Icon size={menu ? 14 : 21} aria-hidden="true" />{menu && <span>{text}</span>}</button>;
  })}</div>;
}
