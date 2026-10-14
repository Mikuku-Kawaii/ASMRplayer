import { useCallback, useEffect, useRef, useState } from "react";
import { callCommand, DesktopLyricsSettingsDto, normalizeAppError, subscribeDesktopLyricsSettings } from "../../lib/api";

export function useLyricsSettings() {
  const [settings, setSettings] = useState<DesktopLyricsSettingsDto | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const revision = useRef(-1);
  const gate = useRef(false);
  const accept = useCallback((s: DesktopLyricsSettingsDto) => { if (s && s.revision >= revision.current) { revision.current = s.revision; setSettings(s); } }, []);
  useEffect(() => {
    let disposed = false;
    let stop: (() => void) | undefined;
    void subscribeDesktopLyricsSettings(s => { if (!disposed) accept(s); }).then(async fn => {
      if (disposed) { fn(); return; } stop = fn;
      const s = await callCommand<DesktopLyricsSettingsDto>("get_desktop_lyrics_settings");
      if (!disposed) accept(s);
    }).catch(e => { if (!disposed) setError(normalizeAppError(e).message); });
    return () => { disposed = true; stop?.(); };
  }, [accept]);
  const update = async (patch: Partial<Omit<DesktopLyricsSettingsDto, "revision">>) => {
    if (gate.current) return false;
    gate.current = true; setBusy(true);
    try { accept(await callCommand<DesktopLyricsSettingsDto>("update_desktop_lyrics_settings", { patch })); setError(""); return true; }
    catch (e) { setError(normalizeAppError(e).message); return false; }
    finally { gate.current = false; setBusy(false); }
  };
  return { settings, update, busy, error, setError };
}
