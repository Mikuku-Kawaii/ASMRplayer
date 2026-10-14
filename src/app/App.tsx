import { useSubtitleFollow } from "../features/lyrics/useSubtitleFollow";
import React, { useEffect, useState, useRef, useCallback } from "react";
import {
  ChevronDown, ExternalLink,
  Headphones,
  Globe,
  Heart,
  Clock,
  Download,
  Settings,
  Minus,
  Square,
  Copy,
  X,
  Pin,
  Play,
  Pause,
  SkipBack,
  SkipForward,
  Maximize2,
  Subtitles,
  ListMusic,
  AppWindow,
  Search,
} from "lucide-react";
import {
  PlaybackSnapshot,
  PlaybackTickDto,
  SubtitleCueDto,
  AutoMatchedSubtitleDto,
  WorkDetailDto,
  defaultSnapshot,
  callCommand,
  formatDurationMs,
  findBestSubtitleInAttachments,
  subscribePlaybackSnapshot,
  subscribePlaybackTick,
  subscribeIslandVisibility,
  subscribeWindowMaximized,
  normalizeAppError,
  AppError,
} from "../lib/api";
import { ErrorBanner } from "../components/ErrorBanner";
import { PlayerBar } from "../features/player/PlayerBar";
import { RightDrawer } from "../features/player/RightDrawer";
import { LibraryView } from "../features/library/LibraryView";
import { WorkDetailView } from "../features/library/WorkDetailView";
import { OnlineView } from "../features/online/OnlineView";
import { DownloadsView } from "../features/downloads/DownloadsView";
import { SettingsView } from "../features/settings/SettingsView";
import { useUpdater } from "../features/settings/useUpdater";
import { ReleaseHistoryHost } from "../features/settings/ReleaseHistory";
import { openUpdateDialog, UpdateDialogHost } from "../features/settings/UpdateDialog";
import { ShortcutProvider, useShortcuts } from "../features/commands/ShortcutProvider";
import { CommandPalette } from "../features/commands/CommandPalette";
import { commands, eventBinding, shortcutTargetBlocked, shortcutTitle } from "../lib/shortcuts";

type NavPage =
  | "library"
  | "online"
  | "favorites"
  | "recent"
  | "downloads"
  | "settings";

export const App: React.FC = () => <ShortcutProvider><AppShell /></ShortcutProvider>;

const AppShell: React.FC = () => {
  const { bindings, ready: shortcutsReady } = useShortcuts();
  const [commandSearchOpen, setCommandSearchOpen] = useState(false);
  const [releaseHistoryOpen, setReleaseHistoryOpen] = useState(false);
  const commandPending = useRef(false);
  const { snapshot: update, run: runUpdate } = useUpdater();
  const [activeNav, setActiveNav] = useState<NavPage>("library");
  const [selectedWorkId, setSelectedWorkId] = useState<string | null>(null);
  const [bookmarkVersion, setBookmarkVersion] = useState<string | undefined>();
  const [snapshot, setSnapshot] = useState<PlaybackSnapshot>(defaultSnapshot);
  const [drawerTab, setDrawerTab] = useState<
    "none" | "queue" | "bookmarks" | "subtitles" | "dsp" | "audio"
  >("none");
  const [globalError, setGlobalError] = useState<AppError | null>(null);
  const [isMaximized, setIsMaximized] = useState<boolean>(false);
  useEffect(() => {
    let disposed = false;
    let eventReceived = false;
    let unlisten: (() => void) | undefined;
    void subscribeWindowMaximized(value => {
      eventReceived = true;
      if (!disposed) setIsMaximized(value);
    }).then(fn => {
      if (disposed) { fn(); return; }
      unlisten = fn;
      return callCommand<boolean>("get_window_maximized").then(value => {
        if (!disposed && !eventReceived && typeof value === "boolean") setIsMaximized(value);
      });
    }).catch(() => {});
    return () => { disposed = true; unlisten?.(); };
  }, []);
  const [isAlwaysOnTop, setIsAlwaysOnTop] = useState<boolean>(false);
  const [libraryRefreshTick, setLibraryRefreshTick] = useState<number>(0);

  // Mini-Player mode & Desktop Island & Immersive Playback Stage state
  const [isIslandOpen, setIsIslandOpen] = useState<boolean>(false);
  const [isMiniMode, setIsMiniMode] = useState<boolean>(false);
  const [isMiniDrawerOpen, setIsMiniDrawerOpen] = useState<boolean>(false);
  const [isImmersiveOpen, setIsImmersiveOpen] = useState<boolean>(false);
  const [immersiveCoverUrl, setImmersiveCoverUrl] = useState<string | null>(
    null
  );
  const [immersiveCues, setImmersiveCues] = useState<SubtitleCueDto[]>([]);
  const [isFetchingStageSubtitles, setIsFetchingStageSubtitles] =
    useState<boolean>(false);
  const [subtitleRefreshTick, setSubtitleRefreshTick] = useState<number>(0);

  // External CV / Tag search payload from WorkDetailView
  const [externalSearch, setExternalSearch] = useState<{
    keyword: string;
    seq: number;
  } | null>(null);

  const mainRef = useRef<HTMLElement | null>(null);
  const detailHistoryRef = useRef<
    Array<{ workId: string | null; scrollTop: number; nav?: NavPage }>
  >([]);

  // Guard eventSeq so stale out-of-order events never overwrite newer playback state
  const lastEventSeqRef = useRef<number>(0);

  const applySnapshot = useCallback((next: PlaybackSnapshot) => {
    if (next.eventSeq >= lastEventSeqRef.current) {
      lastEventSeqRef.current = next.eventSeq;
      setSnapshot(next);
      if (next.lastError) {
        setGlobalError(next.lastError);
      } else if (next.state === "error") {
        setGlobalError({
          code: "MPV_ENGINE_ERROR",
          message: "播放引擎异常中断，可点击「重启引擎」无损恢复当前进度",
          retryable: true,
          hint: null,
        });
      }
    }
  }, []);

  // Lightweight tick handler updates position/duration/state without replacing queue or source arrays
  const applyTick = useCallback((tick: PlaybackTickDto) => {
    if (tick.eventSeq >= lastEventSeqRef.current) {
      lastEventSeqRef.current = tick.eventSeq;
      setSnapshot((prev) => {
        if (tick.sessionGeneration < prev.sessionGeneration) {
          return prev;
        }
        return {
          ...prev,
          eventSeq: tick.eventSeq,
          sessionGeneration: tick.sessionGeneration,
          positionMs: tick.positionMs,
          durationMs: tick.durationMs ?? prev.durationMs,
          state: tick.state,
          sleepTimer:
            tick.sleepRemainingSeconds !== undefined
              ? {
                  ...prev.sleepTimer,
                  remainingSeconds: tick.sleepRemainingSeconds,
                }
              : prev.sleepTimer,
        };
      });
    }
  }, []);

  useEffect(() => {
    let disposed = false;
    let unlistenSnap: (() => void) | null = null;
    let unlistenTick: (() => void) | null = null;
    void callCommand<PlaybackSnapshot>("get_playback_snapshot")
      .then((snap) => { if (!disposed) applySnapshot(snap); })
      .catch(() => {
        // Non-Tauri unit test environment fallback
      });

    void subscribePlaybackSnapshot((snap) => {
      if (!disposed) applySnapshot(snap);
    }).then((fn) => {
      if (disposed) fn(); else unlistenSnap = fn;
    });

    void subscribePlaybackTick((tick) => {
      if (!disposed) applyTick(tick);
    }).then((fn) => {
      if (disposed) fn(); else unlistenTick = fn;
    });

    return () => {
      disposed = true;
      if (unlistenSnap) unlistenSnap();
      if (unlistenTick) unlistenTick();
    };
  }, [applySnapshot, applyTick]);

  // Load work cover & synchronized subtitle cues for Immersive Stage & Mini-Player
  useEffect(() => {
    let cancelled = false;
    setImmersiveCues([]);
    if (!snapshot.currentWorkId) {
      setImmersiveCoverUrl(null);
      setImmersiveCues([]);
      return;
    }
    void callCommand<WorkDetailDto | null>("get_work_detail", {
      workId: snapshot.currentWorkId,
    })
      .then(async (detail) => {
        if (cancelled || !detail) return;
        setImmersiveCoverUrl(detail.coverRef ?? null);

        if (snapshot.currentVersionId) {
          try {
            const autoMatched = await callCommand<AutoMatchedSubtitleDto | null>(
              "auto_match_subtitle_for_version",
              { versionId: snapshot.currentVersionId }
            );
            if (cancelled) return;
            if (autoMatched && Array.isArray(autoMatched.cues)) {
              setImmersiveCues(autoMatched.cues);
              return;
            }
          } catch {
            // Fallback to client-side attachment matching below
          }
        }

        const subAtt = findBestSubtitleInAttachments(
          detail.attachments,
          snapshot.currentVersionId,
          snapshot.currentTrackTitle
        );
        if (subAtt) {
          const cues = await callCommand<SubtitleCueDto[] | null>(
            "load_subtitle_cues",
            {
              attachmentId: subAtt.id,
              offsetMs: subAtt.subtitleOffsetMs ?? 0,
            }
          );
          if (!cancelled && Array.isArray(cues)) {
            setImmersiveCues(cues);
          }
        } else {
          setImmersiveCues([]);
        }
      })
      .catch(() => {
        // Non-Tauri unit test environment fallback
      });
    return () => {
      cancelled = true;
    };
  }, [
    snapshot.currentWorkId,
    snapshot.currentVersionId,
    snapshot.currentTrackTitle,
    subtitleRefreshTick,
  ]);

  const lastMediaSessionPosSyncRef = useRef<number>(0);

  // Sync Web MediaSession metadata & transport/seek action handlers (decoupled from snapshot.state so Pause never resets metadata)
  useEffect(() => {
    if (typeof navigator === "undefined" || !("mediaSession" in navigator)) {
      return;
    }
    const mode = snapshot.smtcMode ?? "full";
    const ms = navigator.mediaSession;
    try {
      if (mode === "disabled" || !snapshot.currentVersionId) {
        ms.metadata = null;
        ms.playbackState = "none";
        return;
      }
      const isPrivacy = mode === "privacy";
      const rawTitle = snapshot.currentTrackTitle || "ASMR Audio Track";
      const cleanTitle =
        rawTitle
          .trim()
          .replace(/\.(mp3|flac|wav|m4a|ogg|opus|aac|wma|ape|alac|aiff|aif|dsf|dff|wv|mka|weba|mp4|mkv|webm|avi|mov|m4v|ts)+$/i, "")
          .trim() || rawTitle;
      const displayTitle = isPrivacy ? "ASMR Audio Track" : cleanTitle;
      const displayArtist = isPrivacy
        ? "ASMR Player"
        : snapshot.currentWorkTitle || "ASMR Player";

      if (typeof MediaMetadata !== "undefined") {
        ms.metadata = new MediaMetadata({
          title: displayTitle,
          artist: displayArtist,
          album: isPrivacy ? "" : snapshot.currentWorkTitle || "",
          artwork:
            !isPrivacy && immersiveCoverUrl
              ? [{ src: immersiveCoverUrl, sizes: "512x512", type: "image/jpeg" }]
              : [],
        });
      }

      ms.setActionHandler("play", () => {
        void callCommand<PlaybackSnapshot>("handle_smtc_action", {
          action: "play",
        }).then(applySnapshot);
      });
      ms.setActionHandler("pause", () => {
        void callCommand<PlaybackSnapshot>("handle_smtc_action", {
          action: "pause",
        }).then(applySnapshot);
      });
      ms.setActionHandler("stop", () => {
        void callCommand<PlaybackSnapshot>("handle_smtc_action", {
          action: "stop",
        }).then(applySnapshot);
      });
      ms.setActionHandler("previoustrack", () => {
        void callCommand<PlaybackSnapshot>("handle_smtc_action", {
          action: "previous",
        }).then(applySnapshot);
      });
      ms.setActionHandler("nexttrack", () => {
        void callCommand<PlaybackSnapshot>("handle_smtc_action", {
          action: "next",
        }).then(applySnapshot);
      });
      ms.setActionHandler("seekto", (details) => {
        if (typeof details.seekTime === "number" && Number.isFinite(details.seekTime)) {
          const targetMs = Math.max(0, Math.round(details.seekTime * 1000));
          lastMediaSessionPosSyncRef.current = 0;
          void callCommand<PlaybackSnapshot>("handle_smtc_action", {
            action: "seek",
            seekMs: targetMs,
          }).then(applySnapshot);
        }
      });
      ms.setActionHandler("seekforward", () => {
        lastMediaSessionPosSyncRef.current = 0;
        void callCommand<PlaybackSnapshot>("handle_smtc_action", {
          action: "fast_forward",
        }).then(applySnapshot);
      });
      ms.setActionHandler("seekbackward", () => {
        lastMediaSessionPosSyncRef.current = 0;
        void callCommand<PlaybackSnapshot>("handle_smtc_action", {
          action: "rewind",
        }).then(applySnapshot);
      });
    } catch {
      // Ignore unsupported mediaSession environments
    }
  }, [
    snapshot.smtcMode,
    snapshot.currentVersionId,
    snapshot.currentTrackTitle,
    snapshot.currentWorkTitle,
    immersiveCoverUrl,
    applySnapshot,
  ]);

  // Sync MediaSession playbackState and timeline position (throttled to ~1s during playback, immediate on state/duration change, preserving position on pause)
  useEffect(() => {
    if (typeof navigator === "undefined" || !("mediaSession" in navigator)) {
      return;
    }
    const mode = snapshot.smtcMode ?? "full";
    const ms = navigator.mediaSession;
    try {
      if (mode === "disabled" || !snapshot.currentVersionId) {
        ms.playbackState = "none";
        return;
      }
      const isPlaying =
        snapshot.state === "playing" ||
        snapshot.state === "loading" ||
        snapshot.state === "buffering" ||
        snapshot.state === "resolving";
      ms.playbackState = isPlaying ? "playing" : "paused";

      if (typeof ms.setPositionState === "function") {
        const durMs = snapshot.durationMs ?? 0;
        if (durMs > 0) {
          const now = Date.now();
          // Always sync immediately on pause/resume or when >= 1000ms elapsed
          if (!isPlaying || now - lastMediaSessionPosSyncRef.current >= 1000) {
            lastMediaSessionPosSyncRef.current = now;
            const durSec = durMs / 1000;
            const posSec = Math.min(durSec, Math.max(0, snapshot.positionMs / 1000));
            ms.setPositionState({
              duration: durSec,
              playbackRate: 1,
              position: posSec,
            });
          }
        }
      }
    } catch {
      // Ignore unsupported setPositionState errors
    }
  }, [
    snapshot.smtcMode,
    snapshot.currentVersionId,
    snapshot.state,
    snapshot.positionMs,
    snapshot.durationMs,
  ]);

  const handleToggleDrawer = (
    tab: "queue" | "bookmarks" | "subtitles" | "dsp" | "audio"
  ) => {
    setDrawerTab((cur) => (cur === tab ? "none" : tab));
  };

  const handleOpenWorkDetail = useCallback(
    (workId: string, versionId?: string) => {
      setBookmarkVersion(versionId);
      setIsImmersiveOpen(false);
      detailHistoryRef.current.push({
        workId: selectedWorkId,
        scrollTop: mainRef.current?.scrollTop ?? 0,
        nav: activeNav,
      });
      setSelectedWorkId(workId);
      if (mainRef.current) {
        mainRef.current.scrollTop = 0;
      }
    },
    [selectedWorkId, activeNav]
  );

  const handleBackFromDetail = useCallback(() => {
    const prev = detailHistoryRef.current.pop();
    if (prev?.nav) {
      setActiveNav(prev.nav);
    }
    setSelectedWorkId(prev?.workId ?? null);
    setLibraryRefreshTick((t) => t + 1);
    const targetScroll = prev?.scrollTop ?? 0;
    requestAnimationFrame(() => {
      if (mainRef.current) {
        mainRef.current.scrollTop = targetScroll;
      }
    });
  }, []);

  // Jump from WorkDetailView CV/Tag pill to search while preserving detail history stack
  const handleSearchKeywordFromDetail = useCallback(
    (keyword: string, preferOnline?: boolean) => {
      const trimmed = keyword.trim();
      if (!trimmed) return;
      setIsImmersiveOpen(false);
      if (selectedWorkId) {
        detailHistoryRef.current.push({
          workId: selectedWorkId,
          scrollTop: mainRef.current?.scrollTop ?? 0,
          nav: activeNav,
        });
      }
      setSelectedWorkId(null);
      const targetNav: NavPage =
        activeNav === "online" || preferOnline ? "online" : "library";
      setActiveNav(targetNav);
      setExternalSearch((prev) => ({
        keyword: trimmed,
        seq: (prev?.seq ?? 0) + 1,
      }));
      if (mainRef.current) {
        mainRef.current.scrollTop = 0;
      }
    },
    [selectedWorkId, activeNav]
  );

  const handleSelectNav = useCallback((page: NavPage) => {
    setIsImmersiveOpen(false);
    detailHistoryRef.current = [];
    setSelectedWorkId(null);
    setActiveNav(page);
    setLibraryRefreshTick((t) => t + 1);
  }, []);

  const commandDisabledReason = useCallback((id: string): string | null => {
    if (id === "play_pause" && !snapshot.currentVersionId && snapshot.queue.length === 0) return "队列为空，请先选择音轨";
    if (["seek_back", "seek_forward"].includes(id) && !snapshot.currentVersionId) return "尚未选择音轨";
    if (["previous", "next"].includes(id) && snapshot.queue.length === 0) return "播放队列为空";
    if (isMiniMode && !["play_pause", "seek_back", "seek_forward", "previous", "next", "volume_up", "volume_down"].includes(id)) return "请先展开完整窗口";
    return null;
  }, [snapshot.currentVersionId, snapshot.queue.length, isMiniMode]);

  const executeCommand = useCallback(async (id: string) => {
    if (commandPending.current) throw new Error("上一条命令正在执行，请稍后重试");
    const reason = commandDisabledReason(id); if (reason) throw new Error(reason);
    if (id === "command_search") { setCommandSearchOpen(true); return; }
    if (["library", "online", "favorites", "downloads", "settings"].includes(id)) { handleSelectNav(id as NavPage); return; }
    if (["queue", "bookmarks", "subtitles"].includes(id)) { setDrawerTab(id as "queue" | "bookmarks" | "subtitles"); return; }
    if (id === "immersive") { setIsImmersiveOpen(value => !value); return; }
    commandPending.current = true;
    try {
      let next: PlaybackSnapshot;
      if (id === "play_pause") next = await callCommand<PlaybackSnapshot>("toggle_pause");
      else if (id === "previous" || id === "next") next = await callCommand<PlaybackSnapshot>("step_queue", { delta: id === "next" ? 1 : -1 });
      else if (id === "seek_back" || id === "seek_forward") next = await callCommand<PlaybackSnapshot>("seek_ms", { positionMs: Math.max(0, Math.min(snapshot.durationMs ?? Infinity, snapshot.positionMs + (id === "seek_forward" ? 5000 : -5000))) });
      else if (id === "volume_up" || id === "volume_down") next = await callCommand<PlaybackSnapshot>("set_volume", { volume: Math.max(0, Math.min(200, snapshot.volume + (id === "volume_up" ? 5 : -5))) });
      else throw new Error("命令不存在，请重新打开命令搜索");
      applySnapshot(next);
    } finally { commandPending.current = false; }
  }, [applySnapshot, commandDisabledReason, handleSelectNav, snapshot.durationMs, snapshot.positionMs, snapshot.volume]);

  useEffect(() => {
    const keydown = (event: KeyboardEvent) => {
      if (event.defaultPrevented || event.repeat || event.isComposing || event.keyCode === 229 || !shortcutsReady || shortcutTargetBlocked(event.target)) return;
      if (event.code === "Escape" && isImmersiveOpen) { event.preventDefault(); setIsImmersiveOpen(false); return; }
      // Native button/link activation must retain Space/Enter and normal navigation.
      if (event.target instanceof Element && event.target.closest('button, a, [role="button"]') && !event.ctrlKey && !event.altKey) return;
      const binding = eventBinding(event); if (!binding) return;
      const command = commands.find(item => bindings[item.id] === binding); if (!command) return;
      event.preventDefault();
      if (!commandPending.current) void executeCommand(command.id).catch(error => setGlobalError(normalizeAppError(error)));
    };
    window.addEventListener("keydown", keydown);
    return () => window.removeEventListener("keydown", keydown);
  }, [bindings, executeCommand, isImmersiveOpen, shortcutsReady]);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void subscribeIslandVisibility((isOpen) => {
      if (!disposed) setIsIslandOpen(isOpen);
    }).then((fn) => {
      if (disposed) fn(); else unlisten = fn;
    }).catch(() => {});
    void callCommand<boolean>("is_island_open")
      .then((open) => {
        if (!disposed && typeof open === "boolean") setIsIslandOpen(open);
      })
      .catch(() => {});
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  const handleToggleIsland = async () => {
    try {
      const next = await callCommand<boolean>("window_toggle_island", {
        open: !isIslandOpen,
      });
      if (typeof next === "boolean") {
        setIsIslandOpen(next);
      } else {
        setIsIslandOpen((prev) => !prev);
      }
    } catch {
      setIsIslandOpen((prev) => !prev);
    }
  };

  const handleToggleMiniMode = async (nextMini: boolean) => {
    setIsMiniMode(nextMini);
    if (!nextMini) {
      setIsMiniDrawerOpen(false);
    } else {
      setIsAlwaysOnTop(true);
    }
    try {
      await callCommand<boolean>("window_set_mini_mode", {
        miniMode: nextMini,
        expandedQueue: false,
      });
    } catch {
      // Non-Tauri environment fallback
    }
  };

  const handleToggleMiniDrawer = async () => {
    const nextOpen = !isMiniDrawerOpen;
    setIsMiniDrawerOpen(nextOpen);
    try {
      await callCommand<boolean>("window_set_mini_mode", {
        miniMode: true,
        expandedQueue: nextOpen,
      });
    } catch {
      // Non-Tauri environment fallback
    }
  };

  const handleFetchOnlineSubtitlesInStage = async () => {
    if (!snapshot.currentWorkId) return;
    setIsFetchingStageSubtitles(true);
    try {
      await callCommand<number>("fetch_online_subtitles_for_work", {
        workId: snapshot.currentWorkId,
      });
      setSubtitleRefreshTick((t) => t + 1);
    } catch (e) {
      setGlobalError(normalizeAppError(e));
    } finally {
      setIsFetchingStageSubtitles(false);
    }
  };

  const handleMinimizeWindow = async () => {
    try {
      await callCommand("window_minimize");
    } catch {
      // Non-Tauri environment fallback
    }
  };

  const handleToggleMaximizeWindow = async () => {
    setIsMaximized((prev) => !prev);
    try {
      const nextMax = await callCommand<boolean>("window_toggle_maximize");
      setIsMaximized(nextMax);
    } catch {
      // Non-Tauri environment fallback
    }
  };

  const handleToggleAlwaysOnTop = async () => {
    const nextPin = !isAlwaysOnTop;
    setIsAlwaysOnTop(nextPin);
    try {
      const confirmed = await callCommand<boolean>("window_set_always_on_top", {
        alwaysOnTop: nextPin,
      });
      setIsAlwaysOnTop(confirmed);
    } catch {
      // Non-Tauri environment fallback
    }
  };

  const handleCloseWindow = async () => {
    try {
      await callCommand("window_close");
    } catch {
      // Non-Tauri environment fallback
    }
  };

  const handleRestartEngine = async () => {
    try {
      const next = await callCommand<PlaybackSnapshot>("restart_player_engine");
      applySnapshot(next);
      setGlobalError(null);
    } catch (e) {
      setGlobalError(normalizeAppError(e));
    }
  };

  const activeImmersiveCueIdx = immersiveCues.reduce((active, cue, i) => cue.startMs <= snapshot.positionMs ? i : active, -1);
  const subtitleFollow = useSubtitleFollow(activeImmersiveCueIdx, snapshot.currentVersionId, isImmersiveOpen);
  const currentActiveCue =
    activeImmersiveCueIdx >= 0 ? immersiveCues[activeImmersiveCueIdx] : null;
  const miniProgressPct =
    snapshot.durationMs && snapshot.durationMs > 0
      ? Math.max(
          0,
          Math.min(100, (snapshot.positionMs / snapshot.durationMs) * 100)
        )
      : 0;
  const canReturnToDetail = detailHistoryRef.current.some(
    (entry) => entry.workId !== null
  );

  return (
    <div
      className={`app-shell ${isMiniMode ? "is-mini-mode" : ""} ${
        isImmersiveOpen ? "is-immersive-open" : ""
      }`}
    >
      <UpdateDialogHost update={update} run={runUpdate} enabled={!isMiniMode && !commandSearchOpen && !releaseHistoryOpen} />
      {commandSearchOpen && <CommandPalette onClose={() => setCommandSearchOpen(false)} onExecute={executeCommand} disabledReason={commandDisabledReason} />}
      <ErrorBanner
        error={globalError}
        onRestartEngine={
          snapshot.state === "error" || globalError?.code?.startsWith("MPV_")
            ? () => void handleRestartEngine()
            : undefined
        }
        onDismiss={() => setGlobalError(null)}
      />

      {/* 网易云风格紧凑矩形迷你悬浮窗 (340px × 88px / 展开抽屉态 340px × 210px, 圆角封面 + 实时内联字幕 + 悬停控制层 + 底部进度细条) */}
      {isMiniMode && (
        <section
          className={`mini-player-shell ${
            isMiniDrawerOpen ? "is-expanded" : ""
          }`}
          aria-label="迷你悬浮窗播放器"
          data-tauri-drag-region
        >
          <div className="mini-player-main" data-tauri-drag-region>
            <div
              className="mini-player-cover-wrap"
              onClick={() => void handleToggleMiniMode(false)}
              title="点击封面展开完整主窗口"
            >
              {immersiveCoverUrl ? (
                <img
                  src={immersiveCoverUrl}
                  alt={snapshot.currentWorkTitle ?? "封面"}
                  className="mini-player-cover"
                  referrerPolicy="no-referrer"
                />
              ) : (
                <div className="mini-player-cover-fallback">
                  <Headphones size={22} />
                </div>
              )}
            </div>

            <div className="mini-player-meta" data-tauri-drag-region>
              <div className="mini-player-track" data-tauri-drag-region>
                {snapshot.currentTrackTitle ?? "尚未加载音轨"}
              </div>
              <div
                className="mini-player-subtitle tabular-nums"
                data-tauri-drag-region
              >
                {currentActiveCue?.text ||
                  (snapshot.currentWorkTitle
                    ? `${snapshot.currentWorkTitle} · ${formatDurationMs(
                        snapshot.positionMs
                      )} / ${formatDurationMs(snapshot.durationMs)}`
                    : "ASMR Player · 迷你助眠模式")}
              </div>
            </div>

            <div className="mini-player-controls">
              <button
                type="button"
                className="btn-icon-sm"
                onClick={() =>
                  void callCommand<PlaybackSnapshot>("step_queue", {
                    delta: -1,
                  }).then(applySnapshot)
                }
                aria-label="迷你窗上一首"
                title="上一首"
              >
                <SkipBack size={14} />
              </button>
              <button
                type="button"
                className="btn-play-main mini-play-btn"
                onClick={() =>
                  void callCommand<PlaybackSnapshot>("toggle_pause").then(
                    applySnapshot
                  )
                }
                aria-label="迷你窗播放或暂停"
                title={snapshot.state === "playing" ? "暂停" : "播放"}
              >
                {snapshot.state === "playing" ? (
                  <Pause size={15} />
                ) : (
                  <Play size={15} />
                )}
              </button>
              <button
                type="button"
                className="btn-icon-sm"
                onClick={() =>
                  void callCommand<PlaybackSnapshot>("step_queue", {
                    delta: 1,
                  }).then(applySnapshot)
                }
                aria-label="迷你窗下一首"
                title="下一首"
              >
                <SkipForward size={14} />
              </button>
              <button
                type="button"
                className={`btn-icon-sm ${isMiniDrawerOpen ? "active" : ""}`}
                onClick={() => void handleToggleMiniDrawer()}
                aria-label="切换迷你窗字幕与队列抽屉"
                aria-pressed={isMiniDrawerOpen}
                title={
                  isMiniDrawerOpen
                    ? "收起迷你字幕与队列抽屉 (340×88)"
                    : "展开迷你字幕与队列抽屉 (340×210)"
                }
              >
                <ListMusic size={14} />
              </button>
              <button
                type="button"
                className={`btn-icon-sm ${isAlwaysOnTop ? "active" : ""}`}
                onClick={() => void handleToggleAlwaysOnTop()}
                aria-label="迷你窗置顶开关"
                aria-pressed={isAlwaysOnTop}
                title={isAlwaysOnTop ? "取消迷你窗置顶" : "置顶迷你窗"}
              >
                <Pin size={13} />
              </button>
              <button
                type="button"
                className="btn-icon-sm"
                onClick={() => void handleToggleMiniMode(false)}
                aria-label="还原完整主窗口"
                title="退出迷你模式并还原主窗口"
              >
                <Maximize2 size={14} />
              </button>
            </div>
          </div>

          {isMiniDrawerOpen && (
            <div
              className="mini-player-drawer"
              aria-label="迷你窗内嵌字幕与队列列表"
            >
              {immersiveCues.length > 0 ? (
                <ul className="mini-drawer-list">
                  {immersiveCues.slice(0, 12).map((cue, idx) => (
                    <li key={cue.index} className="mini-drawer-item">
                      <button
                        type="button"
                        className={`mini-drawer-btn ${
                          idx === activeImmersiveCueIdx ? "is-active" : ""
                        }`}
                        onClick={() =>
                          void callCommand<PlaybackSnapshot>("seek_ms", {
                            positionMs: cue.startMs,
                          }).then(applySnapshot)
                        }
                      >
                        <span className="tabular-nums mini-drawer-time">
                          {formatDurationMs(cue.startMs)}
                        </span>
                        <span className="mini-drawer-text">{cue.text}</span>
                      </button>
                    </li>
                  ))}
                </ul>
              ) : snapshot.queue.length > 0 ? (
                <ul className="mini-drawer-list">
                  {snapshot.queue.map((item, idx) => (
                    <li
                      key={`${item.versionId}-${idx}`}
                      className="mini-drawer-item"
                    >
                      <button
                        type="button"
                        className={`mini-drawer-btn ${
                          item.versionId === snapshot.currentVersionId
                            ? "is-active"
                            : ""
                        }`}
                        onClick={() =>
                          void callCommand<PlaybackSnapshot>("play_version", {
                            versionId: item.versionId,
                            preferredSourceId: null,
                          }).then(applySnapshot)
                        }
                      >
                        <span className="tabular-nums mini-drawer-time">
                          {String(idx + 1).padStart(2, "0")}
                        </span>
                        <span className="mini-drawer-text">
                          {item.trackTitle}
                        </span>
                      </button>
                    </li>
                  ))}
                </ul>
              ) : (
                <div className="mini-drawer-empty">
                  暂无同步字幕或待播队列项
                </div>
              )}
            </div>
          )}

          <div
            className="mini-player-progress-track"
            role="slider"
            aria-label="迷你窗播放进度条"
            aria-valuemin={0}
            aria-valuemax={snapshot.durationMs ?? 0}
            aria-valuenow={snapshot.positionMs}
            tabIndex={0}
            onClick={(e) => {
              if (!snapshot.durationMs || snapshot.durationMs <= 0) return;
              const rect = e.currentTarget.getBoundingClientRect();
              const ratio = Math.max(
                0,
                Math.min(
                  1,
                  (e.clientX - rect.left) / Math.max(1, rect.width)
                )
              );
              const targetMs = Math.round(ratio * snapshot.durationMs);
              void callCommand<PlaybackSnapshot>("seek_ms", {
                positionMs: targetMs,
              }).then(applySnapshot);
            }}
          >
            <div
              className="mini-player-progress-fill"
              style={{ width: `${miniProgressPct.toFixed(1)}%` }}
            />
          </div>
        </section>
      )}

      <header
        className="app-titlebar"
        data-tauri-drag-region
        aria-label="窗口顶栏"
      >
        <div className="titlebar-left" data-tauri-drag-region>
          <img
            src="/asmr-icon.png"
            alt=""
            className="titlebar-logo"
            data-tauri-drag-region
          />
          <span className="titlebar-app-name" data-tauri-drag-region>
            ASMR Player
          </span>
        </div>

        <div className="titlebar-controls">
          <button type="button" className="titlebar-btn" aria-label="搜索命令" title={shortcutTitle("搜索命令", bindings.command_search)} onClick={() => setCommandSearchOpen(true)}><Search size={14} /></button>
          <button
            type="button"
            className={`titlebar-btn ${isIslandOpen ? "active-island active-pin" : ""}`}
            onClick={() => void handleToggleIsland()}
            aria-label="切换桌面灵动岛"
            aria-pressed={isIslandOpen}
            title="切换独立桌面灵动岛 (Dynamic Island)"
          >
            <AppWindow size={13} />
          </button>
          <button
            type="button"
            className={`titlebar-btn ${isAlwaysOnTop ? "active-pin" : ""}`}
            onClick={() => void handleToggleAlwaysOnTop()}
            aria-label="窗口置顶"
            aria-pressed={isAlwaysOnTop}
            title={isAlwaysOnTop ? "取消窗口置顶" : "窗口置顶"}
          >
            <Pin size={13} />
          </button>
          <button
            type="button"
            className="titlebar-btn"
            onClick={() => void handleMinimizeWindow()}
            aria-label="最小化窗口"
            title="最小化"
          >
            <Minus size={14} />
          </button>
          <button
            type="button"
            className="titlebar-btn"
            onClick={() => void handleToggleMaximizeWindow()}
            aria-label="最大化或还原窗口"
            title={isMaximized ? "向下还原" : "最大化"}
          >
            {isMaximized ? <Copy size={12} /> : <Square size={12} />}
          </button>
          <button
            type="button"
            className="titlebar-btn titlebar-btn-close"
            onClick={() => void handleCloseWindow()}
            aria-label="关闭窗口"
            title="关闭窗口"
          >
            <X size={15} />
          </button>
        </div>
      </header>

      <div className="app-upper">
        <nav className="left-sidebar" aria-label="主导航">
          <div className="brand-header">
            <div className="brand-icon-wrap">
              <img
                src="/asmr-icon.png"
                alt="ASMR Player Logo"
                className="brand-icon-img"
              />
            </div>
            <div>
              <div className="brand-title">ASMR Player</div>
            </div>
          </div>

          <ul className="nav-list">
            <li>
              <button
                type="button"
                className={`nav-btn ${
                  activeNav === "library" && !selectedWorkId ? "active" : ""
                }`}
                onClick={() => handleSelectNav("library")}
              >
                <Headphones size={17} />
                <span>我的音频库</span>
              </button>
            </li>
            <li>
              <button
                type="button"
                className={`nav-btn ${
                  activeNav === "online" && !selectedWorkId ? "active" : ""
                }`}
                onClick={() => handleSelectNav("online")}
              >
                <Globe size={17} />
                <span>在线浏览</span>
              </button>
            </li>
            <li>
              <button
                type="button"
                className={`nav-btn ${
                  activeNav === "favorites" && !selectedWorkId ? "active" : ""
                }`}
                onClick={() => handleSelectNav("favorites")}
              >
                <Heart size={17} />
                <span>喜欢</span>
              </button>
            </li>
            <li>
              <button
                type="button"
                className={`nav-btn ${
                  activeNav === "recent" && !selectedWorkId ? "active" : ""
                }`}
                onClick={() => handleSelectNav("recent")}
              >
                <Clock size={17} />
                <span>最近收听</span>
              </button>
            </li>
            <li>
              <button
                type="button"
                className={`nav-btn ${
                  activeNav === "downloads" && !selectedWorkId ? "active" : ""
                }`}
                onClick={() => handleSelectNav("downloads")}
              >
                <Download size={17} />
                <span>离线下载</span>
              </button>
            </li>
            <li>
              <button
                type="button"
                className={`nav-btn ${
                  activeNav === "settings" && !selectedWorkId ? "active" : ""
                }`}
                onClick={() => handleSelectNav("settings")}
              >
                <Settings size={17} />
                <span>设置与诊断</span>
              </button>
            </li>
          </ul>
          <ReleaseHistoryHost enabled={!isMiniMode && !commandSearchOpen} onOpenChange={setReleaseHistoryOpen} />
          {update?.latestVersion && !update.dismissed && ["available", "ready"].includes(update.phase) && (
            <button type="button" className="btn btn-secondary update-nav-notice" onClick={openUpdateDialog}>
              <Download size={16} />{update.phase === "ready" ? "更新已备妥" : `发现新版本 v${update.latestVersion}`}
            </button>
          )}
        </nav>

        {/* 全窗口沉浸式播放舞台 (覆盖除顶栏外的整个软件窗口，含左侧导航栏与主内容区，联动底栏变形) */}
        {isImmersiveOpen && (
          <section
            className="immersive-stage-overlay is-full-window"
            role="dialog"
            aria-label="沉浸式播放舞台"
          >
            {immersiveCoverUrl && (
              <div
                className="immersive-backdrop-blur"
                style={{ backgroundImage: `url(${immersiveCoverUrl})` }}
                aria-hidden="true"
              />
            )}
            <div className="immersive-stage-content">
              <header className="immersive-stage-header">
                <div className="immersive-meta-block">
                  <h2 className="immersive-track-title">
                    {snapshot.currentTrackTitle ?? "尚未加载音轨"}
                  </h2>
                  <div className="immersive-work-subtitle">
                    {snapshot.currentWorkTitle ??
                      "选择作品并点击底栏封面进入沉浸式舞台"}
                  </div>
                </div>
                <div className="track-actions">
                  {snapshot.currentWorkId && (
                    <button
                      type="button"
                      className="btn btn-icon" title="查看作品详情" aria-label="查看作品详情"
                      onClick={() =>
                        snapshot.currentWorkId &&
                        handleOpenWorkDetail(snapshot.currentWorkId)
                      }
                    >
                      <ExternalLink size={19} aria-hidden="true" />
                    </button>
                  )}
                  <button
                    type="button"
                    className="btn btn-icon" title="字幕设置" aria-label="字幕设置"
                    onClick={() => handleToggleDrawer("subtitles")}
                  >
                    <Settings size={19} aria-hidden="true" />
                  </button>
                  <button
                    type="button"
                    className="btn btn-icon" title="收起舞台" aria-label="收起舞台"
                    onClick={() => setIsImmersiveOpen(false)}
                  >
                    <ChevronDown size={21} aria-hidden="true" />
                  </button>
                </div>
              </header>

              <div className="immersive-lyrics-stage" {...subtitleFollow} tabIndex={0} aria-label="同步字幕">
                {immersiveCues.length === 0 ? (
                  <div className="immersive-empty-lyrics">
                    <p>当前音轨暂无已匹配的同步字幕 (.lrc / .srt / .vtt)</p>
                    <div className="track-actions">
                      {snapshot.currentWorkId && (
                        <button
                          type="button"
                          className="btn btn-primary btn-sm"
                          disabled={isFetchingStageSubtitles}
                          onClick={() =>
                            void handleFetchOnlineSubtitlesInStage()
                          }
                        >
                          <Subtitles size={14} />
                          <span>
                            {isFetchingStageSubtitles
                              ? "正在抓取字幕…"
                              : "抓取云端字幕"}
                          </span>
                        </button>
                      )}
                      <button
                        type="button"
                        className="btn btn-secondary btn-sm"
                        onClick={() => handleToggleDrawer("subtitles")}
                      >
                        <Subtitles size={14} />
                        <span>打开字幕抽屉或匹配本地字幕</span>
                      </button>
                    </div>
                  </div>
                ) : (
                  <ul className="immersive-lyrics-list">
                    {immersiveCues.map((cue, idx) => {
                      const isActive = idx === activeImmersiveCueIdx;
                      const isNear =
                        activeImmersiveCueIdx >= 0 &&
                        Math.abs(idx - activeImmersiveCueIdx) <= 2;
                      const rawParts = cue.text
                        .split(/\n| \/\/ /)
                        .map((s) => s.trim())
                        .filter(Boolean);
                      const primaryText = rawParts[0] ?? cue.text;
                      const secondaryText =
                        rawParts.length > 1
                          ? rawParts.slice(1).join(" · ")
                          : null;
                      return (
                        <li
                          key={cue.index} data-cue-index={idx}
                          className={`immersive-lyric-line ${
                            isActive
                              ? "is-active"
                              : isNear
                              ? "is-near"
                              : "is-far"
                          }`}
                        >
                          <button
                            type="button"
                            className="immersive-lyric-btn"
                            onClick={() =>
                              void callCommand<PlaybackSnapshot>("seek_ms", {
                                positionMs: cue.startMs,
                              }).then(applySnapshot)
                            }
                          >
                            <span className="immersive-lyric-time tabular-nums">
                              {formatDurationMs(cue.startMs)}
                            </span>
                            <span className="immersive-lyric-text-stack">
                              <span className="immersive-lyric-primary">
                                {primaryText}
                              </span>
                              {secondaryText && (
                                <span className="immersive-lyric-secondary">
                                  {secondaryText}
                                </span>
                              )}
                            </span>
                          </button>
                        </li>
                      );
                    })}
                  </ul>
                )}
              </div>
            </div>
          </section>
        )}

        <main className={`main-content ${activeNav === "settings" && !selectedWorkId ? "is-settings-page" : ""}`} ref={mainRef}>
          {selectedWorkId && (
            <WorkDetailView
              workId={selectedWorkId}
              focusVersionId={bookmarkVersion}
              onBack={handleBackFromDetail}
              onSnapshotChange={applySnapshot}
              onOpenDownloads={() => {
                detailHistoryRef.current = [];
                setSelectedWorkId(null);
                setActiveNav("downloads");
              }}
              onSearchKeyword={handleSearchKeywordFromDetail}
              onError={setGlobalError}
            />
          )}

          <div
            className={`view-switch-stage ${
              selectedWorkId ? "is-backgrounded" : "is-foreground"
            }`}
            style={{ display: selectedWorkId ? "none" : "block" }}
          >
            {activeNav === "library" ? (
              <LibraryView
                filterMode="all"
                refreshTick={libraryRefreshTick}
                externalSearch={externalSearch}
                canReturnToDetail={canReturnToDetail}
                onReturnToDetail={handleBackFromDetail}
                onSelectWork={handleOpenWorkDetail}
                onSnapshotChange={applySnapshot}
                onOpenDownloads={() => {
                  detailHistoryRef.current = [];
                  setSelectedWorkId(null);
                  setActiveNav("downloads");
                }}
                onError={setGlobalError}
              />
            ) : activeNav === "favorites" ? (
              <LibraryView
                filterMode="favorites"
                refreshTick={libraryRefreshTick}
                canReturnToDetail={canReturnToDetail}
                onReturnToDetail={handleBackFromDetail}
                onSelectWork={handleOpenWorkDetail}
                onSnapshotChange={applySnapshot}
                onOpenDownloads={() => {
                  detailHistoryRef.current = [];
                  setSelectedWorkId(null);
                  setActiveNav("downloads");
                }}
                onError={setGlobalError}
              />
            ) : activeNav === "recent" ? (
              <LibraryView
                filterMode="recent"
                refreshTick={libraryRefreshTick}
                canReturnToDetail={canReturnToDetail}
                onReturnToDetail={handleBackFromDetail}
                onSelectWork={handleOpenWorkDetail}
                onSnapshotChange={applySnapshot}
                onOpenDownloads={() => {
                  detailHistoryRef.current = [];
                  setSelectedWorkId(null);
                  setActiveNav("downloads");
                }}
                onError={setGlobalError}
              />
            ) : activeNav === "online" ? (
              <OnlineView
                externalSearch={externalSearch}
                canReturnToDetail={canReturnToDetail}
                onReturnToDetail={handleBackFromDetail}
                onOpenWorkDetail={handleOpenWorkDetail}
                onOpenDownloads={() => {
                  detailHistoryRef.current = [];
                  setSelectedWorkId(null);
                  setActiveNav("downloads");
                }}
                onError={setGlobalError}
              />
            ) : activeNav === "downloads" ? (
              <DownloadsView
                onSelectWork={handleOpenWorkDetail}
                onError={setGlobalError}
              />
            ) : (
              <SettingsView onError={setGlobalError} />
            )}
          </div>
        </main>

        <RightDrawer
          activeTab={drawerTab}
          onClose={() => setDrawerTab("none")}
          snapshot={snapshot}
          onSnapshotChange={applySnapshot}
          onOpenWorkDetail={handleOpenWorkDetail}
          onSubtitlesChanged={() => setSubtitleRefreshTick((t) => t + 1)}
          onError={setGlobalError}
        />
      </div>

      <PlayerBar
        snapshot={snapshot}
        onSnapshotChange={applySnapshot}
        activeDrawerTab={drawerTab}
        onToggleDrawer={handleToggleDrawer}
        onOpenWorkDetail={handleOpenWorkDetail}
        isImmersiveOpen={isImmersiveOpen}
        onToggleImmersive={() => setIsImmersiveOpen((v) => !v)}
        onToggleMiniMode={() => void handleToggleMiniMode(!isMiniMode)}
        isIslandOpen={isIslandOpen}
        onToggleIsland={handleToggleIsland}
        onError={setGlobalError}
      />
    </div>
  );
};
