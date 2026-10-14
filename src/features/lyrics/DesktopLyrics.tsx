import { CSSProperties, useEffect, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { lyricsFonts } from "./lyricsAppearance";
import { GripHorizontal, Lock, X } from "lucide-react";
import { callCommand, defaultSnapshot, PlaybackSnapshot, SubtitleCueDto, subscribeActiveSubtitles, subscribePlaybackSnapshot, subscribePlaybackTick, normalizeAppError } from "../../lib/api";
import { useLyricsSettings } from "./useLyricsSettings";

export function DesktopLyrics() {
  const { settings, update, error, setError, busy } = useLyricsSettings();
  const [snapshot, setSnapshot] = useState(defaultSnapshot);
  const [subtitles, setSubtitles] = useState<{ versionId: string; cues: SubtitleCueDto[] } | null>(null);
  const seq = useRef(-1);
  async function openMenu(x: number, y: number) {
    if (!settings || busy) return;
    try { await callCommand("desktop_lyrics_open_menu", { x, y }); }
    catch (e) { setError(normalizeAppError(e).message); }
  }
  useEffect(() => {
    let disposed = false;
    const stops: (() => void)[] = [];
    const keep = (fn: () => void) => { if (disposed) fn(); else stops.push(fn); };
    const accept = (s: PlaybackSnapshot) => { if (!disposed && s.eventSeq >= seq.current) { seq.current = s.eventSeq; setSnapshot(s); } };
    void subscribePlaybackSnapshot(accept).then(async fn => { keep(fn); if (!disposed) accept(await callCommand<PlaybackSnapshot>("get_playback_snapshot")); }).catch(e => { if (!disposed) setError(normalizeAppError(e).message); });
    void subscribePlaybackTick(t => { if (!disposed && t.eventSeq >= seq.current) { seq.current = t.eventSeq; setSnapshot(s => ({ ...s, ...t })); } }).then(keep);
    return () => { disposed = true; stops.forEach(fn => fn()); };
  }, [setError]);
  useEffect(() => {
    let disposed = false; let sequence = -1; let stop: (() => void) | undefined;
    const accept = (s: { versionId: string; eventSeq: number; cues: SubtitleCueDto[] }) => {
      if (!disposed && s?.versionId === snapshot.currentVersionId && s.eventSeq >= sequence) { sequence = s.eventSeq; setSubtitles(s); }
    };
    void subscribeActiveSubtitles(accept).then(async fn => { if (disposed) { fn(); return; } stop = fn; accept(await callCommand("get_active_subtitles")); }).catch(e => { if (!disposed) setError(normalizeAppError(e).message); });
    return () => { disposed = true; stop?.(); };
  }, [snapshot.currentVersionId, setError]);
  const loaded = !!settings;
  useEffect(() => { if (loaded) void callCommand("desktop_lyrics_ready").catch(e => setError(normalizeAppError(e).message)); }, [loaded, setError]); // ready once after preferences render
  const cues = subtitles?.versionId === snapshot.currentVersionId ? subtitles?.cues ?? [] : [];
  const cue = [...cues].reverse().find(c => c.startMs <= snapshot.positionMs);
  const lines = (cue?.text ?? snapshot.currentTrackTitle ?? "播放音频后显示字幕").split(/\n|\s*\/\/\s*/).filter(Boolean);
  async function drag() {
    if (!settings || settings.locked || busy) return;
    try { await getCurrentWindow().startDragging(); await callCommand("desktop_lyrics_save_position"); }
    catch (e) { setError(normalizeAppError(e).message); }
  }
  if (!settings) return null;
  return <main tabIndex={0} aria-label="桌面歌词，右键打开菜单" onContextMenu={e => { e.preventDefault(); void openMenu(e.clientX, e.clientY); }} onKeyDown={e => { if (e.key === "ContextMenu" || (e.shiftKey && e.key === "F10")) { e.preventDefault(); e.stopPropagation(); void openMenu(20, 32); } }} className={`desktop-lyrics ${settings.locked ? "is-locked" : ""} lyrics-${settings.color}`} style={{ "--lyrics-size": `${settings.fontSize}px`, "--lyrics-opacity": settings.backgroundEnabled ? settings.backgroundOpacity : 0, "--lyrics-font": (lyricsFonts.find(font => font.value === settings.fontFamily) ?? lyricsFonts[0])?.family ?? "sans-serif" } as CSSProperties}>
    {!settings.locked && <div className="desktop-lyrics-tools">
      <button className="btn btn-icon" disabled={settings.locked || busy} aria-label="拖动桌面歌词" title="拖动桌面歌词" onPointerDown={e => { if (e.button === 0) void drag(); }}><GripHorizontal size={18} /></button>
      <button className="btn btn-icon" disabled={busy} aria-label="锁定桌面歌词" title="锁定当前位置" onClick={() => void update({ locked: true })}><Lock size={16} /></button>
      <button className="btn btn-icon" disabled={busy} aria-label="关闭桌面歌词" title="关闭桌面歌词" onClick={() => void update({ enabled: false })}><X size={18} /></button>
    </div>}
    <div className="desktop-lyrics-text" key={`${snapshot.currentVersionId}-${cue?.index ?? "title"}`}>
      <div>{lines[0]}</div>{settings.showTranslation && lines.length > 1 && <div className="desktop-lyrics-translation">{lines.slice(1).join("\n")}</div>}
    </div>
    {error && <p role="alert" className="desktop-lyrics-error">{error}</p>}
  </main>;
}
