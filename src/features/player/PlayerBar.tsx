import React, { useState, useEffect, useRef } from "react";
import { useShortcuts } from "../commands/ShortcutProvider";
import { shortcutTitle } from "../../lib/shortcuts";
import {
  Play,
  Pause,
  SkipBack,
  SkipForward,
  Rewind,
  FastForward,
  Volume2,
  VolumeX,
  Repeat,
  Repeat1,
  Moon,
  ListMusic,
  Bookmark,
  Subtitles,
  Radio,
  HardDrive,
  CheckCircle2,
  Headphones,
  SlidersHorizontal,
  MoreHorizontal,
  ChevronUp,
  AppWindow,
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
  activeDrawerTab: "none" | "queue" | "bookmarks" | "subtitles" | "dsp" | "audio";
  onToggleDrawer: (tab: "queue" | "bookmarks" | "subtitles" | "dsp" | "audio") => void;
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
  onToggleMiniMode,
  isIslandOpen = false,
  onToggleIsland,
  onError,
}) => {
  const { bindings } = useShortcuts();
  // Optimistic seek slider position while dragging
  const [draggingPosMs, setDraggingPosMs] = useState<number | null>(null);
  const [abPointA, setAbPointA] = useState<number | null>(null);
  const [prevVolume, setPrevVolume] = useState<number>(85);
  const [moreMenuOpen, setMoreMenuOpen] = useState<boolean>(false);
  const [workCoverUrl, setWorkCoverUrl] = useState<string | null>(null);
  const [coverLoadFailed, setCoverLoadFailed] = useState<boolean>(false);
  const [waveformPeaks, setWaveformPeaks] = useState<number[]>(
    DEFAULT_WAVEFORM_PEAKS
  );
  const [trackMetrics, setTrackMetrics] =
    useState<TrackAcousticMetricsDto | null>(null);
  const moreMenuRef = useRef<HTMLDivElement | null>(null);

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

  // Load 64-point waveform envelope & EBU R128 loudness when version changes
  useEffect(() => {
    let cancelled = false;
    if (!snapshot.currentVersionId) {
      setWaveformPeaks(DEFAULT_WAVEFORM_PEAKS);
      setTrackMetrics(null);
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
          setTrackMetrics(metrics);
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

  const handleVolumeChange = async (vol: number) => {
    if (vol > 0) {
      setPrevVolume(vol);
    }
    try {
      const next = await callCommand<PlaybackSnapshot>("set_volume", {
        volume: vol,
      });
      onSnapshotChange(next);
    } catch (e) {
      onError(normalizeAppError(e));
    }
  };

  const handleToggleMute = async () => {
    const targetVol = snapshot.volume > 0 ? 0 : prevVolume > 0 ? prevVolume : 85;
    await handleVolumeChange(targetVol);
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

  const sourceBadge = () => {
    const src = snapshot.activeSource;
    if (!src) return null;
    if (src.sourceKind === "local_imported") {
      return (
        <span className="badge badge-local" title={src.relativePath ?? ""}>
          <HardDrive size={11} />
          <span>本地原件 ({src.format.toUpperCase()})</span>
        </span>
      );
    }
    if (src.sourceKind === "local_downloaded") {
      return (
        <span className="badge badge-downloaded" title={src.relativePath ?? ""}>
          <CheckCircle2 size={11} />
          <span>已下载离线 ({src.format.toUpperCase()})</span>
        </span>
      );
    }
    return (
      <span className="badge badge-online">
        <Radio size={11} />
        <span>在线流 ({src.format.toUpperCase()})</span>
      </span>
    );
  };

  const telemetryBadge = () => {
    const tel = snapshot.audioTelemetry;
    const parts: string[] = [];
    if (tel?.codec) {
      parts.push(tel.codec.toUpperCase());
    }
    if (tel?.sampleRateHz && tel.sampleRateHz > 0) {
      parts.push(`${(tel.sampleRateHz / 1000).toFixed(1)} kHz`);
    }
    if (tel?.bitrateKbps && tel.bitrateKbps > 0) {
      parts.push(`${tel.bitrateKbps} kbps`);
    }
    if (trackMetrics) {
      parts.push(`${trackMetrics.integratedLufs.toFixed(1)} LUFS`);
    }
    if (parts.length === 0) return null;
    return (
      <span
        className="badge badge-tech tabular-nums"
        title={`解码器: ${tel?.codec ?? "默认"} · 采样率: ${
          tel?.sampleRateHz ?? "未知"
        } Hz · 格式: ${tel?.sampleFormat ?? "float"} · 声道: ${
          tel?.channels ?? "stereo"
        }${
          trackMetrics
            ? ` · EBU R128: ${trackMetrics.integratedLufs.toFixed(
                1
              )} LUFS / ${trackMetrics.truePeakDbtp.toFixed(1)} dBTP`
            : ""
        }`}
      >
        {parts.join(" · ")}
      </span>
    );
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

      {/* 左栏：当前作品封面 + 音轨信息与所属作品 */}
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
          <div
            className="player-track-title"
            title={snapshot.currentTrackTitle ?? "尚未加载音轨"}
          >
            {snapshot.currentTrackTitle ?? "尚未加载音轨"}
          </div>
          <div className="player-work-row">
            {snapshot.currentWorkId && snapshot.currentWorkTitle ? (
              <button
                type="button"
                className="link-btn player-work-title"
                onClick={() =>
                  snapshot.currentWorkId &&
                  onOpenWorkDetail?.(snapshot.currentWorkId)
                }
                title={snapshot.currentWorkTitle}
              >
                {snapshot.currentWorkTitle}
              </button>
            ) : (
              <span className="text-muted">选择作品开始收听</span>
            )}
            {sourceBadge()}
            {telemetryBadge()}
          </div>
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
            <Rewind size={16} />
          </button>

          <button
            type="button"
            className="btn-play-main"
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
            <FastForward size={16} />
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
          <button
            type="button"
            className={`btn-icon ${activeDrawerTab === "queue" ? "active-icon-btn" : ""}`}
            onClick={() => onToggleDrawer("queue")}
            aria-label="播放队列"
            aria-expanded={activeDrawerTab === "queue"}
            title={shortcutTitle(`播放队列 (${snapshot.queue.length})`, bindings.queue)}
          >
            <ListMusic size={17} />
          </button>
        </div>

        <div className="player-progress-row">
          <span className="time-label">{formatDurationMs(displayPosMs)}</span>
          <div className="waveform-seekbar-wrap">
            <div className="waveform-bars-bg" aria-hidden="true">
              {waveformPeaks.map((peak, idx) => {
                const barPct = ((idx + 0.5) / waveformPeaks.length) * 100;
                const isPlayed = barPct <= seekProgressPct;
                return (
                  <span
                    key={idx}
                    className={`waveform-bar ${isPlayed ? "is-played" : ""}`}
                    style={{
                      height: `${Math.max(18, Math.round(peak * 100))}%`,
                    }}
                  />
                );
              })}
            </div>
            <input
              type="range"
              className="seek-slider"
              aria-label="播放进度"
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
              onMouseUp={(e) =>
                void handleSeekCommit(
                  Number((e.target as HTMLInputElement).value)
                )
              }
              onTouchEnd={(e) =>
                void handleSeekCommit(
                  Number((e.target as HTMLInputElement).value)
                )
              }
              onKeyUp={(e) => {
                if (e.key === "ArrowLeft" || e.key === "ArrowRight") {
                  void handleSeekCommit(
                    Number((e.target as HTMLInputElement).value)
                  );
                }
              }}
            />
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

      {/* 右栏：收纳为单一触发按钮 + 右侧弹出式二级控制菜单（或点击关闭已打开的抽屉） */}
      <div className="player-right" ref={moreMenuRef}>
        <button
          type="button"
          className={`btn-right-menu-trigger ${
            moreMenuOpen || activeDrawerTab !== "none" || hasActiveSecondaryTool
              ? "active"
              : ""
          }`}
          onClick={handleRightMenuTriggerClick}
          aria-label="更多播放工具与抽屉"
          aria-expanded={moreMenuOpen || activeDrawerTab !== "none"}
          title={
            activeDrawerTab !== "none"
              ? "点击关闭当前右侧面板"
              : "展开右侧控制与音效二级菜单（音量、音效、队列、书签、字幕、睡眠定时）"
          }
        >
          <SlidersHorizontal size={15} />
          <span className="btn-trigger-label">控制与音效</span>
          <span className="trigger-vol-badge">
            {snapshot.volume === 0 ? "静音" : `${Math.round(snapshot.volume)}%`}
          </span>
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
          {/* 1. 音量调节行 */}
          <div className="popover-volume-row">
            <button
              type="button"
              className="btn-icon-sm"
              onClick={() => void handleToggleMute()}
              aria-label={snapshot.volume === 0 ? "取消静音" : "静音"}
              title={
                snapshot.volume === 0
                  ? "取消静音"
                  : `静音 (当前 ${Math.round(snapshot.volume)}%)`
              }
            >
              {snapshot.volume === 0 ? (
                <VolumeX size={15} />
              ) : (
                <Volume2 size={15} />
              )}
            </button>
            <input
              type="range"
              className="volume-slider"
              aria-label="音量"
              min={0}
              max={200}
              value={Math.round(snapshot.volume)}
              style={
                {
                  "--vol-pct": `${Math.max(
                    0,
                    Math.min(100, (snapshot.volume / 200) * 100)
                  ).toFixed(1)}%`,
                } as React.CSSProperties
              }
              onChange={(e) => void handleVolumeChange(Number(e.target.value))}
            />
            <span className="popover-volume-val">
              {Math.round(snapshot.volume)}%
            </span>
          </div>

          <div className="player-more-divider" />
          <div className="player-more-section-label">抽屉面板</div>

          {/* 2. 四大抽屉快捷网格 */}
          <div className="popover-drawer-grid">
            <button
              type="button"
              role="menuitem"
              className={`popover-drawer-btn ${
                activeDrawerTab === "dsp" ? "active" : ""
              }`}
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
              role="menuitem"
              className={`popover-drawer-btn ${
                activeDrawerTab === "audio" ? "active" : ""
              }`}
              onClick={() => {
                onToggleDrawer("audio");
                setMoreMenuOpen(false);
              }}
              aria-label="WASAPI 独占"
              title="打开 WASAPI 独占与输出设备设置"
            >
              <Headphones size={14} />
              <span>WASAPI 独占</span>
            </button>

            <button
              type="button"
              role="menuitem"
              className={`popover-drawer-btn ${
                activeDrawerTab === "bookmarks" ? "active" : ""
              }`}
              onClick={() => {
                onToggleDrawer("bookmarks");
                setMoreMenuOpen(false);
              }}
              aria-label="版本书签与循环"
            >
              <Bookmark size={14} />
              <span>书签与循环面板</span>
            </button>

            <button
              type="button"
              role="menuitem"
              className={`popover-drawer-btn ${
                activeDrawerTab === "subtitles" ? "active" : ""
              }`}
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
              ) : (
                <Repeat size={14} />
              )}
              <span>循环模式</span>
            </span>
            <span className="player-more-item-meta">{loopLabel}</span>
          </button>



          {(onToggleIsland || onToggleMiniMode) && (
            <button
              type="button"
              role="menuitem"
              className={`player-more-item ${isIslandOpen ? "is-active" : ""}`}
              onClick={() => {
                setMoreMenuOpen(false);
                if (onToggleIsland) {
                  onToggleIsland();
                } else if (onToggleMiniMode) {
                  onToggleMiniMode();
                }
              }}
              aria-label="切换桌面灵动岛"
              aria-pressed={isIslandOpen}
            >
              <span className="player-more-item-left">
                <AppWindow size={14} />
                <span>桌面灵动岛 (Dynamic Island)</span>
              </span>
              <span className="player-more-item-meta">
                {isIslandOpen ? "已开启" : "独立桌面悬浮"}
              </span>
            </button>
          )}

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


