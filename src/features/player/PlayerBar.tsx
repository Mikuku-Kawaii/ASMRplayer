import { SeekFiveIcon } from "../../components/SeekFiveIcon";
import { PlayerVolumeControl } from "./PlayerVolumeControl";
import { PlayerLibraryActions } from "./PlayerLibraryActions";
import { DesktopLyricsToggle } from "./DesktopLyricsToggle";
import { useTimelineMarks } from "./timelineMarks";
import React, { useState, useEffect, useRef } from "react";
import { useShortcuts } from "../commands/ShortcutProvider";
import { shortcutTitle } from "../../lib/shortcuts";
import {
  Play,
  Pause,
  SkipBack,
  SkipForward,
  Repeat,
  Repeat1,
  Moon,
  ListMusic,
  MapPin,
  ListEnd,
  Subtitles,
  Radio,
  HardDrive,
  CheckCircle2,
  Headphones,
  SlidersHorizontal,
  MoreHorizontal,
  ChevronUp,
  AudioLines,
  Sparkles,
} from "lucide-react";
import {
  PlaybackSnapshot,
  TrackAcousticMetricsDto,
  formatDurationMs,
  callCommand,
  normalizeAppError,
  AppError,
} from "../../lib/api";

const DEFAULT_WAVEFORM_PEAKS: number[] = Array.from({ length: 64 }, (_, i) => {
  const phase = (i / 64) * Math.PI * 6;
  return Number((0.28 + 0.42 * Math.abs(Math.sin(phase))).toFixed(3));
});

interface PlayerBarProps {
  snapshot: PlaybackSnapshot;
  onSnapshotChange: (snap: PlaybackSnapshot) => void;
  activeDrawerTab: "none" | "queue" | "playlists" | "immersion" | "bookmarks" | "subtitles" | "dsp" | "audio";
  onToggleDrawer: (tab: "queue" | "playlists" | "immersion" | "bookmarks" | "subtitles" | "dsp" | "audio") => void;
  onOpenWorkDetail?: (workId: string) => void;
  isImmersiveOpen?: boolean;
  onToggleImmersive?: () => void;
  onToggleMiniMode?: () => void;
  isIslandOpen?: boolean;
  onToggleIsland?: () => void;
  onError: (err: AppError) => void;
}

export const PlayerBar: React.FC<PlayerBarProps> = ({
  snapshot,
  onSnapshotChange,
  activeDrawerTab,
  onToggleDrawer,
  onOpenWorkDetail,
  isImmersiveOpen = false,
  onToggleImmersive,
  onError,
}) => {
  const { bindings } = useShortcuts();
  // Optimistic seek slider position while dragging
  const [draggingPosMs, setDraggingPosMs] = useState<number | null>(null);
  const [abPointA, setAbPointA] = useState<number | null>(null);
  const [moreMenuOpen, setMoreMenuOpen] = useState<boolean>(false);
  const [workCoverUrl, setWorkCoverUrl] = useState<string | null>(null);
  const [coverLoadFailed, setCoverLoadFailed] = useState<boolean>(false);
  const [waveformPeaks, setWaveformPeaks] = useState<number[]>(
    DEFAULT_WAVEFORM_PEAKS
  );
  const [loudnessBusy, setLoudnessBusy] = useState(false);
  const loudnessPending = useRef(false);
  const moreMenuRef = useRef<HTMLDivElement | null>(null);
  const marksLayer = useRef<HTMLDivElement | null>(null);
  const savedMarks = useTimelineMarks(snapshot.currentVersionId);

  useEffect(() => {
    let cancelled = false;
    setCoverLoadFailed(false);
    if (!snapshot.currentWorkId) {
      setWorkCoverUrl(null);
      return;
    }
    void callCommand<{ coverRef?: string | null } | null>("get_work_detail", {
      workId: snapshot.currentWorkId,
    })
      .then((detail) => {
        if (!cancelled) {
          setWorkCoverUrl(detail?.coverRef ?? null);
        }
      })
      .catch(() => {
        if (!cancelled) {
          setWorkCoverUrl(null);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [snapshot.currentWorkId]);

  // Legacy waveform preview only; its byte estimates are not measured LUFS/true peak.
  useEffect(() => {
    let cancelled = false;
    if (!snapshot.currentVersionId) {
      setWaveformPeaks(DEFAULT_WAVEFORM_PEAKS);
      return;
    }
    void callCommand<TrackAcousticMetricsDto | null>(
      "analyze_track_acoustics",
      {
        versionId: snapshot.currentVersionId,
      }
    )
      .then((metrics) => {
        if (!cancelled && metrics && Array.isArray(metrics.waveformPeaks)) {
          setWaveformPeaks(
            metrics.waveformPeaks.length > 0
              ? metrics.waveformPeaks
              : DEFAULT_WAVEFORM_PEAKS
          );
        }
      })
      .catch(() => {
        // Fallback to default envelope in non-Tauri test environments
      });
    return () => {
      cancelled = true;
    };
  }, [snapshot.currentVersionId]);

  useEffect(() => {
    if (!moreMenuOpen) return;
    const handlePointerDown = (e: MouseEvent) => {
      if (
        moreMenuRef.current &&
        !moreMenuRef.current.contains(e.target as Node)
      ) {
        setMoreMenuOpen(false);
      }
    };
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        setMoreMenuOpen(false);
      }
    };
    window.addEventListener("mousedown", handlePointerDown);
    window.addEventListener("keydown", handleKeyDown);
    return () => {
      window.removeEventListener("mousedown", handlePointerDown);
      window.removeEventListener("keydown", handleKeyDown);
    };
  }, [moreMenuOpen]);

  const displayPosMs =
    draggingPosMs !== null ? draggingPosMs : snapshot.positionMs;
  const maxDurationMs =
    snapshot.durationMs && snapshot.durationMs > 0 ? snapshot.durationMs : 0;
  const clampedPosMs = Math.min(
    displayPosMs,
    maxDurationMs > 0 ? maxDurationMs : 1000
  );
  const marks = maxDurationMs > 0 ? savedMarks.filter(ms => ms >= 0 && ms <= maxDurationMs) : [];
  const seekProgressPct =
    maxDurationMs > 0
      ? Math.max(0, Math.min(100, (clampedPosMs / maxDurationMs) * 100))
      : 0;

  const isPlaying = snapshot.state === "playing";
  const isLoadingOrBuffering =
    snapshot.state === "resolving" ||
    snapshot.state === "loading" ||
    snapshot.state === "buffering";

  const handleTogglePause = async () => {
    try {
      const next = await callCommand<PlaybackSnapshot>("toggle_pause");
      onSnapshotChange(next);
    } catch (e) {
      onError(normalizeAppError(e));
    }
  };

  const handleStep = async (delta: number) => {
    try {
      const next = await callCommand<PlaybackSnapshot>("step_queue", { delta });
      onSnapshotChange(next);
    } catch (e) {
      onError(normalizeAppError(e));
    }
  };

  const handleSeekRelative = async (deltaMs: number) => {
    if (!snapshot.currentVersionId) return;
    const targetMs = Math.max(0, snapshot.positionMs + deltaMs);
    try {
      const next = await callCommand<PlaybackSnapshot>("seek_ms", {
        positionMs: Math.round(targetMs),
      });
      onSnapshotChange(next);
    } catch (e) {
      onError(normalizeAppError(e));
    }
  };

  const handleSeekCommit = async (targetMs: number) => {
    setDraggingPosMs(null);
    try {
      const next = await callCommand<PlaybackSnapshot>("seek_ms", {
        positionMs: Math.round(targetMs),
      });
      onSnapshotChange(next);
    } catch (e) {
      onError(normalizeAppError(e));
    }
  };

  // Use the same range gesture for taps and drags, including taps on a mark.
  const commitPointerSeek = (input: HTMLInputElement) => {
    const requested = input.valueAsNumber;
    const layer = marksLayer.current;
    let target = requested;
    if (layer && marks.length) {
      const width = layer.getBoundingClientRect().width;
      const nearest = marks.reduce((a, b) => Math.abs(b - requested) < Math.abs(a - requested) ? b : a);
      if (width > 0 && Math.abs(nearest - requested) * width / maxDurationMs <= 10) {
        target = nearest;
        if (!window.matchMedia("(prefers-reduced-motion: reduce)").matches) {
          layer.querySelector(`[data-ms="${nearest}"]`)?.animate(
            [{ scale: "1" }, { scale: "1.7", offset: 0.35 }, { scale: "1" }], { duration: 180 }
          );
        }
      }
    }
    void handleSeekCommit(target);
  };

  const handleVolumeChange = async (vol: number) => {
    try {
      const next = await callCommand<PlaybackSnapshot>("set_volume", {
        volume: vol,
      });
      onSnapshotChange(next);
    } catch (e) {
      onError(normalizeAppError(e));
    }
  };


  const handleCycleLoopMode = async () => {
    const order: Array<"none" | "single" | "queue"> = [
      "none",
      "single",
      "queue",
    ];
    const idx = order.indexOf(snapshot.loopMode);
    const nextMode = order[(idx + 1) % order.length] ?? "none";
    try {
      const next = await callCommand<PlaybackSnapshot>("set_loop_mode", {
        loopMode: nextMode,
      });
      onSnapshotChange(next);
    } catch (e) {
      onError(normalizeAppError(e));
    }
  };

  const handleCycleSleepTimer = async () => {
    const cur = snapshot.sleepTimer.mode;
    let nextMode = "off";
    let minutes: number | undefined = undefined;
    if (cur === "off") {
      nextMode = "minutes";
      minutes = 30;
    } else if (cur === "minutes") {
      nextMode = "end_of_track";
    } else {
      nextMode = "off";
    }
    try {
      const next = await callCommand<PlaybackSnapshot>("set_sleep_timer", {
        mode: nextMode,
        minutes,
      });
      onSnapshotChange(next);
    } catch (e) {
      onError(normalizeAppError(e));
    }
  };

  const handleAbButtonClick = async () => {
    try {
      if (snapshot.activeAbLoop) {
        setAbPointA(null);
        const next = await callCommand<PlaybackSnapshot>("clear_ab_loop");
        onSnapshotChange(next);
        return;
      }
      if (abPointA === null) {
        setAbPointA(snapshot.positionMs);
      } else {
        const startMs = Math.min(abPointA, snapshot.positionMs);
        const endMs = Math.max(abPointA, snapshot.positionMs + 500);
        setAbPointA(null);
        const next = await callCommand<PlaybackSnapshot>("set_ab_loop", {
          startMs,
          endMs,
          label: `${formatDurationMs(startMs)} – ${formatDurationMs(endMs)}`,
          saveToDb: true,
        });
        onSnapshotChange(next);
      }
    } catch (e) {
      onError(normalizeAppError(e));
    }
  };

  const handleSwitchSource = async (sourceId: string) => {
    if (!snapshot.currentVersionId) return;
    try {
      const next = await callCommand<PlaybackSnapshot>("play_version", {
        versionId: snapshot.currentVersionId,
        preferredSourceId: sourceId,
      });
      onSnapshotChange(next);
      setMoreMenuOpen(false);
    } catch (e) {
      onError(normalizeAppError(e));
    }
  };

  const audioInfo = () => {
    const tel = snapshot.audioTelemetry;
    const parts: { kind: string; label: string; title?: string }[] = [];
    if (tel?.sourceSampleRateHz && Number.isFinite(tel.sourceSampleRateHz) && tel.sourceSampleRateHz > 0) {
      parts.push({ kind: "sample-rate", label: `${(tel.sourceSampleRateHz / 1000).toLocaleString("en-US", { maximumFractionDigits: 3 })} kHz`, title: "原文件采样率" });
    }
    const bitDepth = tel?.sourceBitDepth;
    if (bitDepth && Number.isInteger(bitDepth) && bitDepth > 0 && bitDepth <= 64) {
      parts.push({ kind: "bit-depth", label: `${bitDepth}-bit`, title: `原文件位深：${bitDepth}-bit` });
    }
    if (tel?.sourceBitrateKbps && Number.isFinite(tel.sourceBitrateKbps) && tel.sourceBitrateKbps > 0) {
      parts.push({ kind: "bitrate", label: `${Math.round(tel.sourceBitrateKbps)} kbps`, title: "原文件平均码率" });
    }
    const summary = parts.map(part => part.label).join(" · ") || (snapshot.currentVersionId ? "音频信息待读取" : "选择作品开始收听");
    const details = [summary];
    if (bitDepth) details.push(`原文件位深：${bitDepth}-bit`);
    if (snapshot.activeSource) {
      details.push(snapshot.activeSource.sourceKind === "local_imported" ? "本地原件" : snapshot.activeSource.sourceKind === "local_downloaded" ? "已下载离线" : "在线流");
    }
    return <div className="player-audio-info tabular-nums" role="group" tabIndex={parts.length ? 0 : undefined} aria-label={`音频信息：${summary}`} title={details.join(" · ")}>
      {snapshot.activeSource && <span
        className={`player-audio-tag player-source-tag ${snapshot.activeSource.sourceKind === "online_provider" ? "is-online" : "is-local"}`}
        title={snapshot.activeSource.sourceKind === "online_provider" ? "在线流" : snapshot.activeSource.sourceKind === "local_downloaded" ? "本地流 · 已下载离线" : "本地流 · 本地导入"}
      >
        {snapshot.activeSource.sourceKind === "online_provider" ? <Radio size={12} aria-hidden="true" /> : <HardDrive size={12} aria-hidden="true" />}
        <span className="ui-tag-label">{snapshot.activeSource.sourceKind === "online_provider" ? "在线流" : "本地流"}</span>
      </span>}
      {parts.length ? parts.map(part => <span key={part.kind} className={`player-audio-tag is-${part.kind}`} title={part.title}><span className="ui-tag-label">{part.label}</span></span>) : summary}
    </div>;
  };

  const loopLabel =
    snapshot.loopMode === "none"
      ? "顺序播放"
      : snapshot.loopMode === "single"
      ? "单曲循环"
      : "队列循环";
  const loopTitle = `切换循环模式（当前：${loopLabel}）`;

  const sleepLabel =
    snapshot.sleepTimer.mode === "off"
      ? "未开启"
      : snapshot.sleepTimer.mode === "end_of_track"
      ? "本轨结束暂停"
      : `剩余 ${formatDurationMs(
          (snapshot.sleepTimer.remainingSeconds ?? 1800) * 1000
        )}`;

  const abStatusLabel = snapshot.activeAbLoop
    ? `循环中 (${snapshot.activeAbLoop.label}) · 点击清除`
    : abPointA !== null
    ? `已设起点 ${formatDurationMs(abPointA)} · 点击设终点 B`
    : "点击设置起点 A";

  const dspActive = !snapshot.dspConfig?.masterBypass;
  const loudnessActive = dspActive && !!snapshot.dspConfig.loudnessNormalization;
  async function toggleLoudness() {
    if (loudnessPending.current) return;
    loudnessPending.current = true;
    setLoudnessBusy(true);
    try {
      onSnapshotChange(await callCommand<PlaybackSnapshot>("set_loudness_normalization", { enabled: !loudnessActive }));
    } catch (error) {
      onError(normalizeAppError(error));
    } finally {
      loudnessPending.current = false;
      setLoudnessBusy(false);
    }
  }
  const hasActiveSecondaryTool =
    Boolean(snapshot.activeAbLoop) ||
    abPointA !== null ||
    snapshot.sleepTimer.mode !== "off" ||
    activeDrawerTab === "bookmarks" ||
    activeDrawerTab === "subtitles";

  const handleRightMenuTriggerClick = () => {
    if (activeDrawerTab !== "none") {
      onToggleDrawer(activeDrawerTab);
      setMoreMenuOpen(false);
      return;
    }
    setMoreMenuOpen((v) => !v);
  };


  return (
    <footer
      className={`player-bar ${isImmersiveOpen ? "is-immersive-mode" : ""}`}
      aria-label="全局播放控制栏"
    >
      {/* 左下方平滑升起的正方形高清大图卡片 (128px × 128px) */}
      <button
        type="button"
        className={`player-rising-cover-card ${
          isImmersiveOpen ? "is-risen" : ""
        }`}
        onClick={() => onToggleImmersive?.()}
        aria-label="收起沉浸式播放舞台"
        title="点击收起沉浸式播放页"
        tabIndex={isImmersiveOpen ? 0 : -1}
      >
        {workCoverUrl && !coverLoadFailed ? (
          <img
            src={workCoverUrl}
            alt={snapshot.currentWorkTitle ?? "当前作品高清封面"}
            className="player-rising-cover-img"
            referrerPolicy="no-referrer"
          />
        ) : (
          <div className="player-rising-cover-fallback">
            <Headphones size={38} />
          </div>
        )}
      </button>

      {/* 左栏：当前作品封面 + 音轨详情入口与音频参数 */}
      <div className="player-left">
        <button
          type="button"
          className={`player-disc-emblem ${isPlaying ? "playing" : ""} ${
            workCoverUrl && !coverLoadFailed ? "has-cover" : ""
          } ${isImmersiveOpen ? "immersive-active" : ""}`}
          onClick={() => {
            if (onToggleImmersive) {
              onToggleImmersive();
            } else if (snapshot.currentWorkId) {
              onOpenWorkDetail?.(snapshot.currentWorkId);
            }
          }}
          disabled={!snapshot.currentWorkId && !snapshot.currentVersionId}
          aria-label="查看当前播放作品详情"
          title={
            isImmersiveOpen
              ? "点击收起沉浸式字幕与封面舞台"
              : snapshot.currentWorkTitle
              ? `点击展开沉浸式字幕舞台：${snapshot.currentWorkTitle}`
              : "尚未加载音轨"
          }
        >
          {workCoverUrl && !coverLoadFailed ? (
            <img
              src={workCoverUrl}
              alt={snapshot.currentWorkTitle ?? "当前作品封面"}
              className="player-cover-img"
              referrerPolicy="no-referrer"
              onError={() => setCoverLoadFailed(true)}
            />
          ) : (
            <Headphones size={20} />
          )}
        </button>

        <div className="player-track-info">
          <div className="player-title-row">
          <button
            type="button"
            className="player-track-title"
            disabled={!snapshot.currentWorkId || !onOpenWorkDetail}
            onClick={() => snapshot.currentWorkId && onOpenWorkDetail?.(snapshot.currentWorkId)}
            title={snapshot.currentTrackTitle ?? "尚未加载音轨"}
          >
            {snapshot.currentTrackTitle ?? "尚未加载音轨"}
          </button>
          </div>
          {audioInfo()}
        </div>
      </div>

      {/* 中栏：对称纯净的传输图标行 + 带 64 点波形包络的时间轴进度条 */}
      <div className="player-center">
        <div className="player-controls-row">
          <button
            type="button"
            className={`btn-icon ${
              snapshot.loopMode !== "none" ? "active-icon-btn" : ""
            }`}
            onClick={() => void handleCycleLoopMode()}
            aria-label={loopTitle}
            title={loopTitle}
          >
            {snapshot.loopMode === "single" ? (
              <Repeat1 size={16} />
            ) : snapshot.loopMode === "none" ? (
              <ListEnd size={16} />
            ) : (
              <Repeat size={16} />
            )}
          </button>
          <button
            type="button"
            className="btn-icon"
            onClick={() => void handleStep(-1)}
            disabled={snapshot.queue.length === 0}
            aria-label="上一首"
            title={shortcutTitle("上一首", bindings.previous)}
          >
            <SkipBack size={17} />
          </button>
          <button
            type="button"
            className="btn-icon"
            onClick={() => void handleSeekRelative(-5000)}
            disabled={!snapshot.currentVersionId}
            aria-label="快退 5 秒"
            title={shortcutTitle("快退 5 秒", bindings.seek_back)}
          >
            <SeekFiveIcon direction="back" />
          </button>
          <button
            type="button"
            className="btn-play-main transport-main"
            onClick={() => void handleTogglePause()}
            disabled={!snapshot.currentVersionId && snapshot.queue.length === 0}
            aria-label={isPlaying ? "暂停" : "播放"}
            title={shortcutTitle(isPlaying ? "暂停" : "播放", bindings.play_pause)}
          >
            {isPlaying ? <Pause size={20} /> : <Play size={20} />}
          </button>
          <button
            type="button"
            className="btn-icon"
            onClick={() => void handleSeekRelative(5000)}
            disabled={!snapshot.currentVersionId}
            aria-label="快进 5 秒"
            title={shortcutTitle("快进 5 秒", bindings.seek_forward)}
          >
            <SeekFiveIcon direction="forward" />
          </button>
          <button
            type="button"
            className="btn-icon"
            onClick={() => void handleStep(1)}
            disabled={snapshot.queue.length === 0}
            aria-label="下一首"
            title={shortcutTitle("下一首", bindings.next)}
          >
            <SkipForward size={17} />
          </button>
          <DesktopLyricsToggle onError={onError} />
        </div>

        <div className="player-progress-row">
          <span className="time-label">{formatDurationMs(displayPosMs)}</span>
          <div className="waveform-seekbar-wrap">
            <div className="waveform-bars-bg" aria-hidden="true">
              {waveformPeaks.map((peak, idx) => (
                <span
                  key={idx}
                  className="waveform-bar"
                  style={{
                    height: `${Math.max(18, Math.round(peak * 100))}%`,
                  }}
                />
              ))}
            </div>
            <input
              type="range"
              className="seek-slider"
              aria-label="播放进度"
              aria-description={marks.length ? `有 ${marks.length} 个时间标记，点击标记可跳转` : undefined}
              min={0}
              max={maxDurationMs > 0 ? maxDurationMs : 1000}
              value={clampedPosMs}
              style={
                {
                  "--seek-pct": `${seekProgressPct.toFixed(2)}%`,
                } as React.CSSProperties
              }
              disabled={!snapshot.currentVersionId}
              onChange={(e) => setDraggingPosMs(Number(e.target.value))}
              onPointerUp={(e) => commitPointerSeek(e.currentTarget)}
              onKeyUp={(e) => {
                if (e.key === "ArrowLeft" || e.key === "ArrowRight") {
                  void handleSeekCommit(
                    Number((e.target as HTMLInputElement).value)
                  );
                }
              }}
            />
            {marks.length > 0 && <div className="timeline-marks" ref={marksLayer} aria-hidden="true">
              {marks.map(ms => <i key={ms} data-ms={ms} style={{ left: `${ms / maxDurationMs * 100}%` }} />)}
            </div>}
          </div>
          <span className="time-label">
            {formatDurationMs(snapshot.durationMs)}
          </span>
          {isLoadingOrBuffering && (
            <span className={`state-pill state-${snapshot.state}`}>
              {snapshot.state === "resolving"
                ? "解析中…"
                : snapshot.state === "loading"
                ? "加载中…"
                : "缓冲中…"}
            </span>
          )}
        </div>
      </div>

      {/* 音量直接可达，其余音效与抽屉入口收纳在工具菜单。 */}
      <div className="player-right" ref={moreMenuRef}>
        <PlayerLibraryActions versionId={snapshot.currentVersionId} localPlayback={snapshot.activeSource?.sourceKind === "local_imported" || snapshot.activeSource?.sourceKind === "local_downloaded"} onError={onError} />
        <button
          type="button"
          className={`btn-icon ${activeDrawerTab === "queue" ? "active-icon-btn" : ""}`}
          data-drawer-trigger="queue"
          onClick={() => { onToggleDrawer("queue"); setMoreMenuOpen(false); }}
          aria-label="播放队列"
          aria-expanded={activeDrawerTab === "queue"}
          title={shortcutTitle(`播放队列 (${snapshot.queue.length})`, bindings.queue)}
        >
          <ListMusic size={17} />
        </button>
        <PlayerVolumeControl volume={snapshot.volume} onChange={handleVolumeChange} />
        <button
          type="button"
          className={`btn-right-menu-trigger ${
            moreMenuOpen || activeDrawerTab !== "none" || hasActiveSecondaryTool
              ? "active"
              : ""
          }`}
          data-drawer-trigger="more"
          onClick={handleRightMenuTriggerClick}
          aria-label="更多播放工具与抽屉"
          aria-expanded={moreMenuOpen || activeDrawerTab !== "none"}
          title={
            activeDrawerTab !== "none"
              ? "点击关闭当前右侧面板"
              : "展开右侧控制与音效二级菜单（歌单、音效、队列、标记、字幕、睡眠定时）"
          }
        >
          <SlidersHorizontal size={15} />
          <span className="btn-trigger-label">控制与音效</span>

          {moreMenuOpen || activeDrawerTab !== "none" ? (
            <ChevronUp size={14} />
          ) : (
            <MoreHorizontal size={14} />
          )}
        </button>

        {/* 右侧弹出的二级菜单面板 */}
        <div
          className={`player-right-popover player-glass-panel ${moreMenuOpen ? "is-open" : ""}`}
          role="menu"
          aria-label="播放辅助工具二级菜单"
        >
          <div role="group" aria-label="歌单与字幕">
            <div className="player-more-section-label">歌单与字幕</div>
            <div className="popover-drawer-grid">
              <button type="button" role="menuitem" className={`popover-drawer-btn ${activeDrawerTab === "playlists" ? "active" : ""}`} data-drawer-trigger="playlists" onClick={() => { onToggleDrawer("playlists"); setMoreMenuOpen(false); }} aria-label="歌单"><ListMusic size={14} /><span>歌单</span></button>
              <button
                type="button"
                role="menuitem"
                className={`popover-drawer-btn ${
                  activeDrawerTab === "subtitles" ? "active" : ""
                }`}
                data-drawer-trigger="subtitles"
                onClick={() => {
                  onToggleDrawer("subtitles");
                  setMoreMenuOpen(false);
                }}
                aria-label="字幕台本"
              >
                <Subtitles size={14} />
                <span>字幕台本</span>
              </button>
            </div>
          </div>
          <div className="player-more-divider" />
          <div role="group" aria-label="音效与输出">
            <div className="player-more-section-label">音效与输出</div>
            <div className="popover-drawer-grid">
              <button
                type="button"
                role="menuitem"
                className={`popover-drawer-btn ${
                  dspActive ? "active" : ""
                }`}
                data-drawer-trigger="dsp"
                onClick={() => {
                  onToggleDrawer("dsp");
                  setMoreMenuOpen(false);
                }}
                aria-label="专业音效与防炸耳 (DSP / EQ)"
                title={`专业音效与防炸耳 (${
                  dspActive
                    ? snapshot.audioTelemetry?.activeDspSummary || "DSP 已开启"
                    : "原声直通"
                })`}
              >
                <SlidersHorizontal size={14} />
                <span>音效</span>
              </button>
              <button
                type="button"
                role="menuitemcheckbox"
                className={`popover-drawer-btn ${loudnessActive ? "active" : ""}`}
                aria-checked={loudnessActive}
                aria-busy={loudnessBusy}
                disabled={loudnessBusy}
                onClick={() => void toggleLoudness()}
                aria-label="响度均衡"
                title="实时调整响度，目标 −23 LUFS，均衡阶段峰值上限 −2 dBTP；会改变动态并以 48 kHz 处理。默认关闭，原声直通时停用。"
              >
                <AudioLines size={14} />
                <span>响度均衡</span>
              </button>
              <button
                type="button"
                role="menuitem"
                className={`popover-drawer-btn ${
                  snapshot.audioExclusive ? "active" : ""
                }`}
                data-drawer-trigger="audio"
                onClick={() => {
                  onToggleDrawer("audio");
                  setMoreMenuOpen(false);
                }}
                aria-label="WASAPI 独占"
                title={`WASAPI 独占${snapshot.audioExclusive ? "已开启，当前设备可能无法供其他程序播放" : "已关闭"}；点击打开输出设备设置`}
              >
                <Headphones size={14} />
                <span>WASAPI 独占</span>
              </button>
              <button type="button" role="menuitem" className={`popover-drawer-btn ${activeDrawerTab === "immersion" ? "active" : ""}`} data-drawer-trigger="immersion" onClick={() => { onToggleDrawer("immersion"); setMoreMenuOpen(false); }} aria-label="沉浸设置"><Sparkles size={14} /><span>沉浸设置</span></button>
            </div>
          </div>

          <div className="player-more-divider" />
          <div className="player-more-section-label">快捷控制</div>

          <button
            type="button"
            role="menuitem"
            className={`player-more-item ${
              snapshot.sleepTimer.mode !== "off" ? "active" : ""
            }`}
            onClick={() => void handleCycleSleepTimer()}
          >
            <span className="player-more-item-left">
              <Moon size={14} />
              <span>睡眠定时</span>
            </span>
            <span className="player-more-item-meta">{sleepLabel}</span>
          </button>

          <button
            type="button"
            role="menuitem"
            className={`player-more-item ${
              snapshot.activeAbLoop || abPointA !== null ? "active" : ""
            }`}
            disabled={!snapshot.currentVersionId}
            onClick={() => void handleAbButtonClick()}
          >
            <span className="player-more-item-left">
              <Repeat size={14} />
              <span>A–B 区间循环</span>
            </span>
            <span className="player-more-item-meta">{abStatusLabel}</span>
          </button>

          <button type="button" role="menuitem" className={`player-more-item ${activeDrawerTab === "bookmarks" ? "active" : ""}`}
            data-drawer-trigger="bookmarks" aria-label="版本标记与循环" onClick={() => { onToggleDrawer("bookmarks"); setMoreMenuOpen(false); }}>
            <span className="player-more-item-left"><MapPin size={14} /><span>标记与循环面板</span></span>
          </button>

          <button
            type="button"
            role="menuitem"
            className={`player-more-item ${
              snapshot.loopMode !== "none" ? "active" : ""
            }`}
            onClick={() => void handleCycleLoopMode()}
          >
            <span className="player-more-item-left">
              {snapshot.loopMode === "single" ? (
                <Repeat1 size={14} />
              ) : snapshot.loopMode === "none" ? (
                <ListEnd size={14} />
              ) : (
                <Repeat size={14} />
              )}
              <span>循环模式</span>
            </span>
            <span className="player-more-item-meta">{loopLabel}</span>
          </button>




          {snapshot.alternativeSources.length > 0 && (
            <>
              <div className="player-more-divider" />
              <div className="player-more-section-label">
                切换同一版本来源 ({snapshot.alternativeSources.length})
              </div>
              {snapshot.alternativeSources.map((alt) => (
                <button
                  key={alt.id}
                  type="button"
                  role="menuitem"
                  className="player-more-item"
                  onClick={() => void handleSwitchSource(alt.id)}
                >
                  <span className="player-more-item-left">
                    {alt.sourceKind === "local_imported" ? (
                      <HardDrive size={14} />
                    ) : alt.sourceKind === "local_downloaded" ? (
                      <CheckCircle2 size={14} />
                    ) : (
                      <Radio size={14} />
                    )}
                    <span>
                      {alt.sourceKind === "local_imported"
                        ? "本地原件"
                        : alt.sourceKind === "local_downloaded"
                        ? "已下载离线"
                        : "在线流"}
                    </span>
                  </span>
                  <span className="player-more-item-meta">
                    {alt.format.toUpperCase()}
                  </span>
                </button>
              ))}
            </>
          )}
        </div>
      </div>
    </footer>
  );
};


