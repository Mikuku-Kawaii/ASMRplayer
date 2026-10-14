import { useEffect } from "react";
import { Captions } from "lucide-react";
import { AppError } from "../../lib/api";
import { useLyricsSettings } from "../lyrics/useLyricsSettings";

export function DesktopLyricsToggle({ onError }: { onError: (error: AppError) => void }) {
  const { settings, update, busy, error } = useLyricsSettings();
  useEffect(() => {
    if (error) onError({ code: "DESKTOP_LYRICS_ERROR", message: error, retryable: true });
  }, [error, onError]);
  const enabled = settings?.enabled ?? false;
  const label = enabled ? "关闭桌面歌词" : "开启桌面歌词";
  return <button type="button" className={`btn btn-icon player-lyrics-toggle ${enabled ? "active" : ""}`}
    title={label} aria-label={label} aria-pressed={enabled} disabled={!settings || busy}
    onClick={() => void update({ enabled: !enabled })}>
    <Captions size={18} aria-hidden="true" />
  </button>;
}
