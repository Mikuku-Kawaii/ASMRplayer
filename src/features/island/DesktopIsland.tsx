import React, { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Play, Pause, SkipBack, SkipForward, Volume2, Disc3, ExternalLink, EyeOff, Eye, Pin, PinOff, X, ChevronUp, ChevronLeft, MoreHorizontal } from "lucide-react";
import {
  PlaybackSnapshot, SubtitleCueDto, WorkDetailDto, AutoMatchedSubtitleDto, AppSettingsDto,
  IslandForm, IslandLayout, IslandPreferences, formatDurationMs, callCommand,
  subscribePlaybackSnapshot, subscribePlaybackTick, subscribeIslandFullscreen,
  subscribeIslandLayout, subscribeIslandGlass, subscribeSettingsUpdated, defaultSnapshot, isTauriRuntime, normalizeAppError,
} from "../../lib/api";

const initialLayout: IslandLayout = { revision: 0, form: "compact", scale: 1, width: 180, height: 42 };
export function DesktopIsland() {
  const [snapshot, setSnapshot] = useState(defaultSnapshot);
  const [nativeGlass, setNativeGlass] = useState(false);
  const [layout, setLayout] = useState(initialLayout);
  const [settings, setSettings] = useState<Partial<AppSettingsDto>>({});
  const [cues, setCues] = useState<SubtitleCueDto[]>([]);
  const [cover, setCover] = useState<string | null>(null);
  const [error, setError] = useState("");
  const [avoided, setAvoided] = useState(false);
  const [toast, setToast] = useState<{ text: string; volume?: number } | null>(null);
  const [seekPreview, setSeekPreview] = useState<number | null>(null);
  const toastTimer = useRef<ReturnType<typeof setTimeout>>();
  const previousVersion = useRef<string | null>(null);
  const sequence = useRef(-1);
  const layoutSequence = useRef(-1);
  const layoutQueue = useRef(Promise.resolve());
  const dragging = useRef(false);
  const pointerStart = useRef<{ x: number; y: number } | null>(null);
  const textMeasure = useRef<HTMLSpanElement>(null);
  const compactWidth = useRef(180);
  const requestedForm = useRef<IslandForm>("compact");
  const form = layout.form;
  const privacy = settings.islandPrivacyMode ?? false;
  const playing = ["playing", "loading", "buffering", "resolving"].includes(snapshot.state);

  const applyLayout = useCallback((next: IslandLayout) => {
    if (next && next.revision >= layoutSequence.current) { layoutSequence.current = next.revision; setLayout(next); if (next.nativeGlass) setNativeGlass(true); }
  }, []);
  const changeForm = useCallback((next: IslandForm, width?: number) => {
    requestedForm.current = next;
    clearTimeout(toastTimer.current);
    setError("");
    layoutQueue.current = layoutQueue.current.then(async () => {
      try { applyLayout(await callCommand<IslandLayout>("island_set_form", { form: next, animate: !window.matchMedia?.("(prefers-reduced-motion: reduce)").matches, ...(next === "compact" ? { compactWidth: width ?? compactWidth.current } : {}) })); }
      catch (e) { setError(normalizeAppError(e).message); }
    });
  }, [applyLayout]);
  useEffect(() => {
    if (!layout.animate) return;
    // Trim the input region after the morph without resizing the WebView viewport.
    // The host ignores obsolete revisions if another cue/form arrives first.
    const timer = setTimeout(() => {
      void callCommand("island_settle_layout", { revision: layout.revision }).catch(e => setError(normalizeAppError(e).message));
    }, 450);
    return () => clearTimeout(timer);
  }, [layout.revision, layout.animate]);
  const run = useCallback(async (command: string, args?: Record<string, unknown>) => {
    try { await callCommand(command, args); setError(""); return true; }
    catch (e) { setError(normalizeAppError(e).message); return false; }
  }, []);

  useEffect(() => {
    let disposed = false;
    const cleanups: (() => void)[] = [];
    const keep = (promise: Promise<() => void>) => void promise.then(fn => disposed ? fn() : cleanups.push(fn)).catch(() => {});
    const applySnapshot = (next: PlaybackSnapshot) => {
      if (!disposed && next.eventSeq >= sequence.current) { sequence.current = next.eventSeq; setSnapshot(next); }
    };
    const snapshots = subscribePlaybackSnapshot(applySnapshot);
    keep(snapshots);
    keep(subscribePlaybackTick(tick => {
      if (!disposed && tick.eventSeq >= sequence.current) {
        sequence.current = tick.eventSeq;
        setSnapshot(prev => ({ ...prev, ...tick }));
      }
    }));
    keep(subscribeIslandGlass(enabled => { if (!disposed) setNativeGlass(enabled); }));
    keep(subscribeIslandLayout(next => { if (!disposed) applyLayout(next); }));
    keep(subscribeIslandFullscreen(next => { if (!disposed) setAvoided(next); }));
    keep(subscribeSettingsUpdated(next => { if (!disposed) setSettings(next); }));
    void snapshots.then(() => callCommand<PlaybackSnapshot>("get_playback_snapshot")).then(applySnapshot).catch(() => {});
    void callCommand<AppSettingsDto>("get_app_settings").then(next => { if (!disposed && next) setSettings(next); }).catch(() => {});
    void callCommand<IslandPreferences>("get_island_preferences").then(next => { if (!disposed && next) applyLayout(next.layout); }).catch(() => {});
    return () => { disposed = true; cleanups.forEach(fn => fn()); clearTimeout(toastTimer.current); };
  }, [applyLayout]);

  useEffect(() => {
    let disposed = false;
    setCover(null); setCues([]); setSeekPreview(null);
   
    if (snapshot.currentWorkId && !privacy) {
      void callCommand<WorkDetailDto | null>("get_work_detail", { workId: snapshot.currentWorkId })
        .then(detail => { if (!disposed) setCover(detail?.coverRef ?? null); }).catch(() => {});
      if (snapshot.currentVersionId) void callCommand<AutoMatchedSubtitleDto | null>("auto_match_subtitle_for_version", { versionId: snapshot.currentVersionId })
        .then(result => { if (!disposed) setCues(result?.cues ?? []); }).catch(() => {});
    }
    return () => { disposed = true; };
  }, [snapshot.currentWorkId, snapshot.currentVersionId, privacy]);

  const showToast = useCallback((payload: { text: string; volume?: number }) => {
    if (form === "expanded" || form === "menu" || settings.islandToastEnabled === false) return;
    setToast(payload);
    changeForm("toast");
    toastTimer.current = setTimeout(() => { changeForm("compact"); setToast(null); }, (settings.islandToastDurationSec ?? 3) * 1000);
  }, [form, settings.islandToastEnabled, settings.islandToastDurationSec, changeForm]);
  useEffect(() => {
    if (snapshot.currentVersionId && previousVersion.current && snapshot.currentVersionId !== previousVersion.current) {
      showToast({ text: privacy ? "已切换音轨" : snapshot.currentTrackTitle || "已切换音轨" });
    }
    previousVersion.current = snapshot.currentVersionId ?? null;
  }, [snapshot.currentVersionId, snapshot.currentTrackTitle, privacy, showToast]);
  useEffect(() => {
    const onBlur = () => { if (!dragging.current && requestedForm.current !== "compact") changeForm("compact"); };
    window.addEventListener("blur", onBlur);
    return () => window.removeEventListener("blur", onBlur);
  }, [changeForm]);

  useEffect(() => {
    const media = window.matchMedia?.("(prefers-reduced-motion: reduce)");
    const update = () => changeForm(requestedForm.current);
    media?.addEventListener?.("change", update);
    return () => media?.removeEventListener?.("change", update);
  }, [changeForm]);

  const position = seekPreview ?? snapshot.positionMs;
  const cueIndex = cues.findIndex(c => position >= c.startMs && position <= c.endMs);
  const cue = cues[cueIndex];
  const lines = privacy ? [] : (cue?.text ?? "").split(/\n| \/\/ /).map(s => s.trim()).filter(Boolean);
  const primaryText = privacy ? "ASMR Audio Track" : lines[0] || snapshot.currentTrackTitle || "ASMR Player";
  const subtitleText = lines[0] || "";
  useLayoutEffect(() => {
    // Measure only when the cue changes, never on each playback tick.
    if (textMeasure.current) textMeasure.current.textContent = subtitleText;
    const width = subtitleText ? Math.min(308, Math.max(148, Math.ceil(textMeasure.current?.scrollWidth ?? 0) + 94)) : 180;
    if (textMeasure.current) textMeasure.current.textContent = "";
    if (compactWidth.current === width) return;
    compactWidth.current = width;
    if (form === "compact" && requestedForm.current === "compact") changeForm("compact", width);
  }, [subtitleText, form, changeForm]);
  async function wheelVolume(e: React.WheelEvent) {
    if (settings.islandScrollVolume === false || e.deltaY === 0) return;
    const volume = Math.max(0, Math.min(200, snapshot.volume + (e.deltaY < 0 ? 5 : -5)));
    if (await run("set_volume", { volume })) showToast({ text: `音量 ${volume}%`, volume });
  }
  function dragStart(e: React.PointerEvent) {
    if (e.button !== 0 || (e.target as HTMLElement).closest("button, input")) return;
    pointerStart.current = { x: e.screenX, y: e.screenY };
  }
  async function dragMove(e: React.PointerEvent) {
    if ((e.buttons & 1) === 0) { pointerStart.current = null; return; }
    const start = pointerStart.current;
    if (!start || dragging.current || !isTauriRuntime() || Math.hypot(e.screenX - start.x, e.screenY - start.y) < 5) return;
    pointerStart.current = null; dragging.current = true;
    try { await getCurrentWindow().startDragging(); await run("island_save_current_position"); }
    catch (e) { setError(normalizeAppError(e).message); }
    finally { dragging.current = false; }
  }
  const commitSeek = () => {
    if (seekPreview !== null) void run("seek_ms", { positionMs: seekPreview });
    setSeekPreview(null);
  };
  async function toggleSetting(key: "islandPrivacyMode" | "islandAlwaysOnTop") {
    const value = !(settings[key] ?? (key === "islandAlwaysOnTop"));
    const command = key === "islandPrivacyMode" ? "island_set_privacy_mode" : "island_set_always_on_top";
    if (await run(command, key === "islandPrivacyMode" ? { enabled: value } : { alwaysOnTop: value })) {
      setSettings(prev => ({ ...prev, [key]: value })); changeForm("compact");
    }
  }
  const coverImage = !privacy && cover ? <img src={cover} alt="作品封面" draggable={false} /> : <Disc3 size={20} />;
  // Native title popups escape this transparent window and can outlive a morph.
  // Keep descriptions accessible without creating separate opaque tooltip HWNDs.
  return <div className={`desktop-island-container form-${form} ${nativeGlass ? "has-native-glass" : ""} ${avoided ? "is-fullscreen-avoided" : ""}`}
    style={{ zoom: layout.scale } as React.CSSProperties}
    onContextMenu={e => { e.preventDefault(); changeForm("menu"); }}
    onKeyDown={e => { if (e.key === "Escape") { changeForm("compact"); } }}>
    <span ref={textMeasure} className="island-text-measure" aria-hidden="true" />
    <div className={`island-capsule island-capsule-${form}`} style={{ width: layout.width, height: layout.height, borderRadius: layout.radius, transitionDuration: layout.animate === false ? "0ms" : undefined }}
      onPointerDown={dragStart} onPointerMove={e => void dragMove(e)} onPointerUp={() => { pointerStart.current = null; }} onPointerCancel={() => { pointerStart.current = null; }}
      onDoubleClick={e => { if (!(e.target as HTMLElement).closest("button, input")) void run("island_focus_main_window"); }}
      role={form === "expanded" || form === "menu" ? "dialog" : "region"}
      aria-label={form === "compact" ? "桌面灵动岛胶囊" : form === "expanded" ? "灵动岛展开控制舱" : form === "menu" ? "灵动岛菜单" : "灵动岛瞬态通知"}
      tabIndex={form === "compact" || form === "toast" ? 0 : -1}
      onKeyDown={e => { if (e.target === e.currentTarget && (e.key === "Enter" || e.key === " ")) { e.preventDefault(); changeForm("expanded"); } }}
      onClick={() => { if ((form === "compact" || form === "toast") && !dragging.current) changeForm("expanded"); }}
      onWheel={form === "compact" || form === "toast" ? e => void wheelVolume(e) : undefined}>
      <div key={form} className={`island-content island-content-${form}`} style={{ width: layout.width - 2, height: layout.height - 2 }}>
      {form === "compact" && <>
        <div className={`island-avatar-wrap ${playing ? "island-cover-breathing" : "is-paused"}`}>{coverImage}</div>
        <span className="island-compact-text">{primaryText}</span>
        <div aria-hidden="true" className={`island-waveform-bars ${playing ? "is-dancing" : "is-frozen"}`}>{[1, 2, 3, 4].map(n => <span key={n} className={`island-bar bar-${n}`} />)}</div>
      </>}
      {form === "expanded" && <>
        <div className="island-header">
          <div className="island-cover-large-wrap">{coverImage}</div>
          <div className="island-subtitles-stage">
            <div className="island-sub-primary" aria-description={privacy ? "ASMR Audio Track" : snapshot.currentTrackTitle || "ASMR Player"}>{privacy ? "ASMR Audio Track" : snapshot.currentTrackTitle || "ASMR Player"}</div><div className="island-sub-secondary">{privacy ? "隐私模式" : snapshot.currentWorkTitle || "选择音轨开始收听"}</div>
          </div>
          <button type="button" className="island-btn-icon" aria-label="收起灵动岛" aria-description="收起灵动岛 (Esc)" onClick={() => changeForm("compact")}><ChevronUp size={16} /></button>
        </div>
        <div className="island-timeline tabular-nums">
          <span>{formatDurationMs(position)}</span>
          <input type="range" aria-label="播放进度" aria-description={`${formatDurationMs(position)} / ${formatDurationMs(snapshot.durationMs)}`} min={0} max={snapshot.durationMs || 1} step={1000} value={position} disabled={!snapshot.durationMs}
            onChange={e => setSeekPreview(Number(e.target.value))}
            onPointerDown={e => e.currentTarget.setPointerCapture?.(e.pointerId)} onPointerUp={commitSeek}
            onPointerCancel={() => setSeekPreview(null)} onKeyUp={commitSeek} onBlur={commitSeek}
            style={{ "--island-progress": `${Math.min(100, Math.max(0, position / (snapshot.durationMs || 1) * 100))}%` } as React.CSSProperties} />
          <span>{formatDurationMs(snapshot.durationMs)}</span>
        </div>
        <div className="island-controls-row">
          <button type="button" className="island-btn-icon" aria-label="更多操作" aria-description="更多操作" onClick={() => changeForm("menu")}><MoreHorizontal size={18} /></button>
          <div className="island-transport">
            <button type="button" className="island-btn-icon" aria-label="上一首" aria-description="上一首" disabled={!snapshot.currentVersionId} onClick={() => void run("step_queue", { delta: -1 })}><SkipBack size={22} /></button>
            <button type="button" className="island-btn-play-main" aria-label={playing ? "暂停" : "播放"} aria-description={playing ? "暂停" : "播放"} disabled={!snapshot.currentVersionId} onClick={() => void run("toggle_pause")}>{playing ? <Pause size={24} /> : <Play size={24} />}</button>
            <button type="button" className="island-btn-icon" aria-label="下一首" aria-description="下一首" disabled={!snapshot.currentVersionId} onClick={() => void run("step_queue", { delta: 1 })}><SkipForward size={22} /></button>
          </div>
          <button type="button" className="island-btn-icon" aria-label="打开主软件" aria-description="打开主软件" onClick={() => void run("island_focus_main_window")}><ExternalLink size={16} /></button>
        </div>
      </>}
      {form === "toast" && <div className="island-toast-content" role="status">{toast?.volume !== undefined ? <Volume2 size={16} /> : <Disc3 size={16} />}<span>{privacy && toast?.volume === undefined ? "已切换音轨" : toast?.text}</span></div>}
      {form === "menu" && <div className="island-context-menu">
        <div className="island-menu-heading"><span>灵动岛</span><button type="button" className="island-btn-icon" aria-description="返回播放控制" aria-label="返回播放控制" onClick={() => changeForm("expanded")}><ChevronLeft size={16} /></button></div>
        <button type="button" className="island-menu-item" onClick={() => void run("island_focus_main_window")}><ExternalLink size={14} />打开主软件</button>
        <button type="button" className="island-menu-item" onClick={() => void toggleSetting("islandPrivacyMode")}>{privacy ? <Eye size={14} /> : <EyeOff size={14} />}{privacy ? "关闭脱敏模式" : "脱敏模式"}</button>
        <button type="button" className="island-menu-item" onClick={() => void toggleSetting("islandAlwaysOnTop")}>{settings.islandAlwaysOnTop !== false ? <PinOff size={14} /> : <Pin size={14} />}{settings.islandAlwaysOnTop !== false ? "取消置顶" : "窗口置顶"}</button>
        <button type="button" className="island-menu-item" onClick={() => void run("window_toggle_island", { open: false })}><X size={14} />关闭灵动岛</button>
      </div>}
      {error && <div className="island-error" role="alert" aria-description={error}>{error}</div>}
      </div>
    </div>
  </div>;
}

