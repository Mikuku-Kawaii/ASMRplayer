import React, { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { ScrollingText } from "./ScrollingText";
import { PlayerLibraryActions } from "../player/PlayerLibraryActions";
import { Play, Pause, SkipBack, SkipForward, Volume2, Volume1, Disc3, ExternalLink, EyeOff, Eye, X, ChevronUp, ChevronLeft, MoreHorizontal, RotateCcw } from "lucide-react";
import {
  PlaybackSnapshot, SubtitleCueDto, WorkDetailDto, AutoMatchedSubtitleDto, AppSettingsDto, AppError,
  IslandForm, IslandLayout, IslandPreferences, formatDurationMs, callCommand,
  subscribePlaybackSnapshot, subscribePlaybackTick, subscribeIslandFullscreen,
  subscribeIslandLayout, subscribeIslandGlass, subscribeSettingsUpdated, subscribeActiveSubtitles, defaultSnapshot, isTauriRuntime, normalizeAppError,
} from "../../lib/api";

// Keep these bounds aligned with services/island.rs; the host clamps all requests.
const COMPACT_MIN_WIDTH = 200;
const COMPACT_MAX_WIDTH = 640;
const initialLayout: IslandLayout = { revision: 0, form: "compact", scale: 1, width: COMPACT_MIN_WIDTH, height: 42, radius: 21, animate: false };
export function DesktopIsland() {
  const [snapshot, setSnapshot] = useState(defaultSnapshot);
  const [nativeGlass, setNativeGlass] = useState(false);
  const [layout, setLayout] = useState(initialLayout);
  const [settings, setSettings] = useState<Partial<AppSettingsDto>>({});
  const [cues, setCues] = useState<SubtitleCueDto[]>([]);
  const [cover, setCover] = useState<string | null>(null);
  const [error, setError] = useState("");
  const reportLibraryError = useCallback((next: AppError) => setError(next.message), []);
  const [avoided, setAvoided] = useState(false);
  const [toast, setToast] = useState<{ text: string; volume?: number } | null>(null);
  const [seekPreview, setSeekPreview] = useState<number | null>(null);
  const toastTimer = useRef<ReturnType<typeof setTimeout>>();
  const previousVersion = useRef<string | null>(null);
  const sequence = useRef(-1);
  const volumeRef = useRef(defaultSnapshot.volume);
  const volumeQueue = useRef(Promise.resolve());
  const layoutSequence = useRef(-1);
  const layoutQueue = useRef(Promise.resolve());
  const dragging = useRef(false);
  const pointerStart = useRef<{ x: number; y: number } | null>(null);
  const textMeasure = useRef<HTMLSpanElement>(null);
  const compactWidth = useRef(COMPACT_MIN_WIDTH);
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

  const applySnapshot = useCallback((next: PlaybackSnapshot) => {
    if (next.eventSeq >= sequence.current) {
      sequence.current = next.eventSeq;
      volumeRef.current = next.volume;
      setSnapshot(next);
    }
  }, []);

  useEffect(() => {
    let disposed = false;
    const cleanups: (() => void)[] = [];
    const keep = (promise: Promise<() => void>) => void promise.then(fn => disposed ? fn() : cleanups.push(fn)).catch(() => {});
    const receiveSnapshot = (next: PlaybackSnapshot) => { if (!disposed) applySnapshot(next); };
    const snapshots = subscribePlaybackSnapshot(receiveSnapshot);
    keep(snapshots);
    keep(subscribePlaybackTick(tick => {
      if (!disposed && tick.eventSeq >= sequence.current) {
        sequence.current = tick.eventSeq;
        setSnapshot(prev => ({ ...prev, ...tick }));
      }
    }));
    keep(subscribeIslandGlass(enabled => { if (!disposed) setNativeGlass(enabled); }));
    const layouts = subscribeIslandLayout(next => { if (!disposed) applyLayout(next); });
    keep(layouts);
    keep(subscribeIslandFullscreen(next => { if (!disposed) setAvoided(next); }));
    keep(subscribeSettingsUpdated(next => { if (!disposed) setSettings(next); }));
    void snapshots.then(() => callCommand<PlaybackSnapshot>("get_playback_snapshot")).then(receiveSnapshot).catch(() => {});
    void callCommand<AppSettingsDto>("get_app_settings").then(next => { if (!disposed && next) setSettings(next); }).catch(() => {});
    // Register before reading: initialization runs concurrently with this WebView.
    // Otherwise its only layout event can fall between the read and subscription.
    void layouts.then(() => disposed ? null : callCommand<IslandPreferences>("get_island_preferences")).then(next => { if (!disposed && next) applyLayout(next.layout); }).catch(() => {});
    return () => { disposed = true; cleanups.forEach(fn => fn()); clearTimeout(toastTimer.current); };
  }, [applyLayout, applySnapshot]);

  useEffect(() => {
    let disposed = false;
    setCover(null); setCues([]); setSeekPreview(null);
    let stop: (() => void) | undefined;
    let subtitleSequence = -1;
   
    if (snapshot.currentWorkId && !privacy) {
      void callCommand<WorkDetailDto | null>("get_work_detail", { workId: snapshot.currentWorkId })
        .then(detail => { if (!disposed) setCover(detail?.coverRef ?? null); }).catch(() => {});
      if (snapshot.currentVersionId) void subscribeActiveSubtitles(next => {
        if (!disposed && next.versionId === snapshot.currentVersionId && next.eventSeq >= subtitleSequence) {
          subtitleSequence = next.eventSeq; setCues(next.cues);
        }
      }).then(unlisten => {
        if (disposed) { unlisten(); return null; }
        stop = unlisten;
        return callCommand<AutoMatchedSubtitleDto | null>("auto_match_subtitle_for_version", { versionId: snapshot.currentVersionId });
      }).then(result => { if (!disposed && subtitleSequence < 0) setCues(result?.cues ?? []); }).catch(() => {});
    }
    return () => { disposed = true; stop?.(); };
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
  // Keep the latest started cue through gaps. Derive it from the current
  // position so seeking backwards also restores the correct cue (or title).
  const cue = cues.reduce<SubtitleCueDto | undefined>((latest, candidate) =>
    candidate.startMs <= position && (!latest || candidate.startMs > latest.startMs) ? candidate : latest, undefined);
  const lines = privacy ? [] : (cue?.text ?? "").split(/\n| \/\/ /).map(s => s.trim()).filter(Boolean);
  const subtitleText = lines[0] || "";
  const hasSubtitles = !privacy && cues.length > 0;
  const source = snapshot.activeSource ?? snapshot.alternativeSources[0];
  const sourcePath = source?.relativePath || source?.remoteRelativePath || source?.absolutePath;
  const fileName = sourcePath?.split(/[\\/]/).filter(Boolean).pop() || snapshot.currentTrackTitle?.trim() || "";
  const workTitle = snapshot.currentWorkTitle?.trim() || "";
  const titleText = [...new Set([workTitle, fileName].filter(Boolean))].join(" · ") || "ASMR Player";
  const primaryText = privacy ? "ASMR Audio Track" : hasSubtitles
    ? lines[0] || snapshot.currentTrackTitle || "ASMR Player" : titleText;
  const expandedTitle = privacy || hasSubtitles ? primaryText : workTitle || fileName || "ASMR Player";
  const secondaryText = privacy ? "隐私模式" : hasSubtitles
    ? lines[1] || workTitle || "选择音轨开始收听"
    : workTitle && fileName !== workTitle ? fileName : "";
  const textIdentity = `${snapshot.currentVersionId}:${hasSubtitles ? cue?.index ?? "gap" : "title"}:${privacy}`;
  const [compactText, setCompactText] = useState({ text: primaryText, identity: textIdentity });
  useEffect(() => {
    if (layout.revision <= 0) return;
    // Acknowledge only after React has committed the exact host layout. Hidden
    // WebViews can throttle RAF indefinitely, so use a post-commit task instead.
    const timer = setTimeout(() => {
      void callCommand("island_layout_ready", { revision: layout.revision }).catch(e => setError(normalizeAppError(e).message));
    }, 0);
    return () => clearTimeout(timer);
  }, [layout.revision]);
  useLayoutEffect(() => {
    // Measure only when the cue changes, never on each playback tick.
    if (textMeasure.current) textMeasure.current.textContent = subtitleText;
    // Symmetric 28px side columns + gaps, padding and capsule border keep text centered.
    const width = subtitleText ? Math.min(COMPACT_MAX_WIDTH, Math.max(COMPACT_MIN_WIDTH, Math.ceil(textMeasure.current?.scrollWidth ?? 0) + 98)) : COMPACT_MIN_WIDTH;
    if (textMeasure.current) textMeasure.current.textContent = "";
    if (compactWidth.current === width) return;
    compactWidth.current = width;
    if (form === "compact" && requestedForm.current === "compact") changeForm("compact", width);
  }, [subtitleText, hasSubtitles, form, changeForm]);
  useLayoutEffect(() => {
    // Wait for the matching host width so text entry and capsule spring start in
    // the same React commit. Privacy masking must never wait for the host.
    if (privacy || error || (form === "compact" && layout.width === compactWidth.current)) {
      setCompactText(previous => previous.text === primaryText && previous.identity === textIdentity
        ? previous : { text: primaryText, identity: textIdentity });
    }
  }, [primaryText, textIdentity, privacy, error, form, layout.width]);
  function adjustVolume(delta: number) {
    // Serialize rapid clicks so each step starts from the last confirmed host volume.
    volumeQueue.current = volumeQueue.current.then(async () => {
      const volume = Math.max(0, Math.min(200, volumeRef.current + delta));
      if (volume === volumeRef.current) return;
      try {
        const next = await callCommand<PlaybackSnapshot>("set_volume", { volume });
        applySnapshot(next);
        setError("");
        showToast({ text: `音量 ${next.volume}%`, volume: next.volume });
      } catch (e) { setError(normalizeAppError(e).message); }
    });
  }
  function wheelVolume(e: React.WheelEvent) {
    if (settings.islandScrollVolume === false || e.deltaY === 0) return;
    adjustVolume(e.deltaY < 0 ? 5 : -5);
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
  async function togglePrivacy() {
    const value = !privacy;
    if (await run("island_set_privacy_mode", { enabled: value })) {
      setSettings(prev => ({ ...prev, islandPrivacyMode: value })); changeForm("compact");
    }
  }
  const coverImage = !privacy && cover ? <img src={cover} alt="作品封面" draggable={false} /> : <Disc3 size={20} />;
  return <div className={`desktop-island-container form-${form} ${nativeGlass ? "has-native-glass" : ""} ${avoided ? "is-fullscreen-avoided" : ""}`}
    onContextMenu={e => { e.preventDefault(); changeForm("menu"); }}
    onKeyDown={e => { if (e.key === "Escape") { changeForm("compact"); } }}>
    <span ref={textMeasure} className="island-text-measure" aria-hidden="true" />
    <div className={`island-capsule island-capsule-${form}`} style={{ width: layout.width, height: layout.height, borderRadius: layout.radius, transform: `scale(${layout.scale})`, transformOrigin: "top center", transitionDuration: layout.animate === false ? "0ms" : undefined }}
      onPointerDown={dragStart} onPointerMove={e => void dragMove(e)} onPointerUp={() => { pointerStart.current = null; }} onPointerCancel={() => { pointerStart.current = null; }}
      onDoubleClick={e => { if (!(e.target as HTMLElement).closest("button, input")) void run("island_focus_main_window"); }}
      role={form === "expanded" || form === "menu" ? "dialog" : "region"}
      aria-label={form === "compact" ? "桌面灵动岛胶囊" : form === "expanded" ? "灵动岛展开控制舱" : form === "menu" ? "灵动岛菜单" : "灵动岛瞬态通知"}
      tabIndex={form === "compact" || form === "toast" ? 0 : -1}
      onKeyDown={e => { if (e.target === e.currentTarget && (e.key === "Enter" || e.key === " ")) { e.preventDefault(); changeForm("expanded"); } }}
      onClick={() => { if ((form === "compact" || form === "toast") && !dragging.current) changeForm("expanded"); }}
      onWheel={form === "compact" || form === "toast" ? e => void wheelVolume(e) : undefined}>
      <div className={`island-cover-motion ${form === "expanded" ? "island-cover-large-wrap" : "island-avatar-wrap"} ${form === "compact" && playing ? "island-cover-breathing" : ""}`}
        aria-hidden={form !== "compact" && form !== "expanded" ? true : undefined}
        style={{ transitionDuration: layout.animate === false ? "0ms" : undefined }}>{coverImage}</div>
      <div aria-hidden="true" className={`island-waveform-bars island-waveform-motion ${playing ? "is-dancing" : "is-frozen"}`}
        style={{ transitionDuration: layout.animate === false ? "0ms" : undefined }}>{[1, 2, 3, 4, 5, 6].map(n => <span key={n} className={`island-bar bar-${n}`} />)}</div>
      <div className="island-compact-text-motion" aria-hidden={form !== "compact" ? true : undefined}
        style={{ transitionDuration: layout.animate === false ? "0ms" : undefined }}>
        <ScrollingText className="island-compact-text" text={privacy ? primaryText : compactText.text} identity={privacy ? textIdentity : compactText.identity} loop={!hasSubtitles} />
      </div>
      <div key={form} className={`island-content island-content-${form}`} style={{ width: layout.width - 2, height: layout.height - 2 }}>
      {form === "expanded" && <>
        <div className="island-header">
          <div aria-hidden="true" />
          <div className="island-subtitles-stage">
            <ScrollingText className="island-sub-primary" text={expandedTitle} identity={textIdentity} loop={!hasSubtitles} />
            {secondaryText && (!privacy && !hasSubtitles
              ? <ScrollingText className="island-sub-secondary" text={secondaryText} identity={`${textIdentity}:filename`} loop />
              : <div key={`${textIdentity}:${secondaryText}`} className="island-sub-secondary island-text-transition" title={secondaryText}>{secondaryText}</div>)}
          </div>
          <button type="button" className="island-btn-icon" aria-label="收起灵动岛" aria-description="收起灵动岛 (Esc)" onClick={() => changeForm("compact")}><ChevronUp size={16} /></button>
        </div>
        <div className="island-timeline tabular-nums">
          <span>{formatDurationMs(position)}</span>
          <div className="island-seek" style={{ "--island-progress": `${Math.min(100, Math.max(0, position / (snapshot.durationMs || 1) * 100))}%` } as React.CSSProperties}>
          <div className="island-progress-track" aria-hidden="true"><span /></div>
          <input type="range" aria-label="播放进度" aria-valuetext={`${formatDurationMs(position)} / ${formatDurationMs(snapshot.durationMs)}`} min={0} max={snapshot.durationMs || 1} step={1000} value={Math.min(position, snapshot.durationMs || 1)} disabled={!snapshot.durationMs}
            onChange={e => setSeekPreview(Number(e.target.value))}
            onPointerDown={e => e.currentTarget.setPointerCapture?.(e.pointerId)} onPointerUp={commitSeek}
            onPointerCancel={() => setSeekPreview(null)} onKeyUp={commitSeek} onBlur={commitSeek} />
          </div>
          <span>{formatDurationMs(snapshot.durationMs)}</span>
        </div>
        <div className="island-controls-row">
          <button type="button" className="island-btn-icon" aria-label="更多操作" title="更多操作" onClick={() => changeForm("menu")}><MoreHorizontal size={24} /></button>
          <div className="island-transport">
            <button type="button" className="island-btn-icon" aria-label="上一首" title="上一首" disabled={!snapshot.currentVersionId} onClick={() => void run("step_queue", { delta: -1 })}><SkipBack size={30} fill="currentColor" strokeWidth={2.5} /></button>
            <button type="button" className="island-btn-play-main" aria-label={playing ? "暂停" : "播放"} title={playing ? "暂停" : "播放"} disabled={!snapshot.currentVersionId} onClick={() => void run("toggle_pause")}>{playing ? <Pause size={32} fill="currentColor" strokeWidth={1.5} /> : <Play size={32} fill="currentColor" strokeWidth={1.5} />}</button>
            <button type="button" className="island-btn-icon" aria-label="下一首" title="下一首" disabled={!snapshot.currentVersionId} onClick={() => void run("step_queue", { delta: 1 })}><SkipForward size={30} fill="currentColor" strokeWidth={2.5} /></button>
          </div>
          <button type="button" className="island-btn-icon" aria-label="打开主软件" title="打开主软件查看完整标题与字幕" onClick={() => void run("island_focus_main_window")}><ExternalLink size={22} /></button>
        </div>
      </>}
      {form === "toast" && <div className="island-toast-content" role="status">{toast?.volume !== undefined ? <Volume2 size={16} /> : <Disc3 size={16} />}<span>{privacy && toast?.volume === undefined ? "已切换音轨" : toast?.text}</span></div>}
      {form === "menu" && <div className="island-context-menu">
        <div className="island-menu-heading"><span>灵动岛</span><button type="button" className="island-btn-icon" aria-description="返回播放控制" aria-label="返回播放控制" onClick={() => changeForm("expanded")}><ChevronLeft size={16} /></button></div>
        <div className="island-menu-columns">
        <div className="island-menu-column">
        <PlayerLibraryActions versionId={snapshot.currentVersionId} onError={reportLibraryError} variant="island-menu" />
        <button type="button" className="island-menu-item" aria-pressed={privacy} onClick={() => void togglePrivacy()}>{privacy ? <Eye size={14} /> : <EyeOff size={14} />}<span className="ui-icon-label">{privacy ? "关闭脱敏模式" : "脱敏模式"}</span></button>
        </div>
        <div className="island-menu-column" role="group" aria-label="灵动岛与音量">
        <button type="button" className="island-menu-item" aria-label="提高音量" aria-description="每次提高 5 点音量" onClick={() => adjustVolume(5)}><Volume2 size={14} /><span className="ui-icon-label">提高音量</span></button>
        <button type="button" className="island-menu-item" aria-label="降低音量" aria-description="每次降低 5 点音量" onClick={() => adjustVolume(-5)}><Volume1 size={14} /><span className="ui-icon-label">降低音量</span></button>
        <button type="button" className="island-menu-item" title="恢复到所选屏幕顶部居中位置" onClick={() => {
          layoutQueue.current = layoutQueue.current.then(async () => {
            try { applyLayout(await callCommand<IslandLayout>("island_reset_position")); setError(""); }
            catch (e) { setError(normalizeAppError(e).message); }
          });
        }}><RotateCcw size={14} /><span className="ui-icon-label">恢复原位</span></button>
        <button type="button" className="island-menu-item" onClick={() => void run("window_toggle_island", { open: false })}><X size={14} /><span className="ui-icon-label">关闭灵动岛</span></button>
        </div>
        </div>
      </div>}
      {error && <div className="island-error" role="alert" aria-description={error}>{error}</div>}
      </div>
    </div>
  </div>;
}

