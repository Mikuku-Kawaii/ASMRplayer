import { AudioOutputPanel } from "./AudioOutputPanel";
import { BookmarkNotes } from "./BookmarkNotes";
import { QueuePlans } from "./QueuePlans";
import { Select } from "../../components/Select";
import { subtitleWindows } from "../../lib/subtitleWindow";
import React, { useEffect, useState, useCallback, useMemo } from "react";
import {
  Play,
  Trash2,
  ArrowUp,
  ArrowDown,
  Plus,
  Clock,
  X,
  SlidersHorizontal,
  ShieldAlert,
  Sparkles,
  RotateCcw,
  Upload,
} from "lucide-react";
import {
  PlaybackSnapshot,
  AbLoopDto,
  SubtitleCueDto,
  AutoMatchedSubtitleDto,
  AttachmentDto,
  WorkDetailDto,
  DspConfigDto,
  DspPresetDto,
  DspStateDto,
  defaultDspConfig,
  builtinDspPresets,
  computeFrequencyResponseCurve,
  findBestSubtitleInAttachments,
  callCommand,
  formatDurationMs,
  normalizeAppError,
  AppError,
} from "../../lib/api";

interface RightDrawerProps {
  activeTab: "none" | "queue" | "bookmarks" | "subtitles" | "dsp" | "audio";
  onClose: () => void;
  snapshot: PlaybackSnapshot;
  onSnapshotChange: (snap: PlaybackSnapshot) => void;
  onSubtitlesChanged?: () => void;
  onOpenWorkDetail?: (workId: string, versionId?: string) => void;
  onError: (err: AppError) => void;
}

const LOG_FREQS_HZ: number[] = Array.from({ length: 48 }, (_, i) => {
  const t = i / 47;
  return Math.round(20 * Math.pow(1000, t));
});

export const RightDrawer: React.FC<RightDrawerProps> = ({
  activeTab,
  onClose,
  snapshot,
  onSnapshotChange,
  onSubtitlesChanged,
  onOpenWorkDetail,
  onError,
}) => {
  const [savedLoops, setSavedLoops] = useState<AbLoopDto[]>([]);
  const [sleepMinutesInput, setSleepMinutesInput] = useState(30);
  const [sleepFadeOutSec, setSleepFadeOutSec] = useState<number>(
    snapshot.sleepTimer?.fadeOutSeconds ?? 30
  );

  const [subtitles, setSubtitles] = useState<AttachmentDto[]>([]);
  const [selectedSubId, setSelectedSubId] = useState<string>("");
  const [subtitleOffsetMs, setSubtitleOffsetMs] = useState<number>(0);
  const [cues, setCues] = useState<SubtitleCueDto[]>([]);
  const [subtitleKeyword, setSubtitleKeyword] = useState<string>("");
  const [fetchingCloudSubs, setFetchingCloudSubs] = useState<boolean>(false);
  // Windowed rendering limits for huge queues and 1,000+ line subtitle files
  const [visibleQueueLimit, setVisibleQueueLimit] = useState<number>(100);
  const [visibleCueLimit, setVisibleCueLimit] = useState<number>(150);

  // DSP / EQ Drawer state & Interactive PEQ Canvas state
  const [dspConfig, setDspConfig] = useState<DspConfigDto>(
    snapshot.dspConfig ?? defaultDspConfig
  );
  const [activePresetId, setActivePresetId] = useState<string | null>(
    snapshot.activeDspPresetId ?? "asmr_pure_guard"
  );
  const [dspPresets, setDspPresets] = useState<DspPresetDto[]>(builtinDspPresets);
  const [workOverrideEnabled, setWorkOverrideEnabled] = useState<boolean>(false);
  const [showAutoEqImport, setShowAutoEqImport] = useState<boolean>(false);
  const [autoEqTextInput, setAutoEqTextInput] = useState<string>(
    "Preamp: -4.2 dB\nFilter 1: ON LSC Fc 105 Hz Gain 4.0 dB Q 0.70\nFilter 2: ON PK Fc 3200 Hz Gain -2.8 dB Q 2.10\nFilter 3: ON PK Fc 6500 Hz Gain -3.5 dB Q 3.80"
  );
  const [customPresetName, setCustomPresetName] = useState<string>("");
  const [draggingBandIdx, setDraggingBandIdx] = useState<number | null>(null);

  useEffect(() => {
    if (snapshot.dspConfig) {
      setDspConfig(snapshot.dspConfig);
    }
    if (snapshot.activeDspPresetId !== undefined) {
      setActivePresetId(snapshot.activeDspPresetId ?? null);
    }
    if (snapshot.sleepTimer?.fadeOutSeconds !== undefined) {
      setSleepFadeOutSec(snapshot.sleepTimer.fadeOutSeconds);
    }
  }, [snapshot.dspConfig, snapshot.activeDspPresetId, snapshot.sleepTimer?.fadeOutSeconds]);

  const loadDspState = useCallback(async () => {
    try {
      const state = await callCommand<DspStateDto>("get_dsp_state", {
        workId: snapshot.currentWorkId ?? null,
      });
      setDspConfig(state.config);
      setActivePresetId(state.activePresetId ?? null);
      setDspPresets(state.presets.length > 0 ? state.presets : builtinDspPresets);
      setWorkOverrideEnabled(state.workOverrideEnabled);
    } catch {
      // Fallback to snapshot/default in non-Tauri test environments
    }
  }, [snapshot.currentWorkId]);

  const loadBookmarksAndLoops = useCallback(async () => {
    if (!snapshot.currentVersionId) {
      setSavedLoops([]);
      return;
    }
    try {
      const loops = await callCommand<AbLoopDto[]>("list_ab_loops", { versionId: snapshot.currentVersionId });
      setSavedLoops(loops);
    } catch (e) {
      onError(normalizeAppError(e));
    }
  }, [snapshot.currentVersionId, onError]);

  const loadWorkSubtitles = useCallback(async () => {
    if (!snapshot.currentWorkId) {
      setSubtitles([]);
      setCues([]);
      return;
    }
    try {
      let autoMatched: AutoMatchedSubtitleDto | null = null;
      if (snapshot.currentVersionId) {
        try {
          autoMatched = await callCommand<AutoMatchedSubtitleDto | null>(
            "auto_match_subtitle_for_version",
            { versionId: snapshot.currentVersionId }
          );
        } catch {
          autoMatched = null;
        }
      }

      const detail = await callCommand<WorkDetailDto>("get_work_detail", {
        workId: snapshot.currentWorkId,
      });
      const subs = detail.attachments.filter((a) => a.attachmentType === "subtitle");
      setSubtitles(subs);

      if (autoMatched && Array.isArray(autoMatched.cues)) {
        setSelectedSubId(autoMatched.attachment.id);
        setSubtitleOffsetMs(autoMatched.attachment.subtitleOffsetMs);
        setCues(autoMatched.cues);
        return;
      }

      const bound =
        findBestSubtitleInAttachments(
          subs,
          snapshot.currentVersionId,
          snapshot.currentTrackTitle
        ) ?? subs[0];
      if (bound) {
        setSelectedSubId(bound.id);
        setSubtitleOffsetMs(bound.subtitleOffsetMs);
        const loadedCues = await callCommand<SubtitleCueDto[]>("load_subtitle_cues", {
          attachmentId: bound.id,
        });
        setCues(loadedCues);
      } else {
        setSelectedSubId("");
        setCues([]);
      }
    } catch (e) {
      onError(normalizeAppError(e));
    }
  }, [
    snapshot.currentWorkId,
    snapshot.currentVersionId,
    snapshot.currentTrackTitle,
    onError,
  ]);

  useEffect(() => {
    if (activeTab === "bookmarks") {
      void loadBookmarksAndLoops();
    } else if (activeTab === "subtitles") {
      void loadWorkSubtitles();
    } else if (activeTab === "dsp") {
      void loadDspState();
    }
  }, [activeTab, loadBookmarksAndLoops, loadWorkSubtitles, loadDspState]);

  const freqCurveDb = useMemo(
    () => computeFrequencyResponseCurve(dspConfig, LOG_FREQS_HZ),
    [dspConfig]
  );

  if (activeTab === "none") return null;

  const handlePlayQueueItem = async (versionId: string, preferredSourceId?: string | null) => {
    try {
      const next = await callCommand<PlaybackSnapshot>("play_version", {
        versionId,
        preferredSourceId: preferredSourceId ?? null,
      });
      onSnapshotChange(next);
    } catch (e) {
      onError(normalizeAppError(e));
    }
  };

  const handleRemoveQueueItem = async (queueItemId: string) => {
    try {
      const next = await callCommand<PlaybackSnapshot>("remove_from_queue", {
        queueItemId,
      });
      onSnapshotChange(next);
    } catch (e) {
      onError(normalizeAppError(e));
    }
  };

  const handleClearQueue = async () => {
    try {
      const next = await callCommand<PlaybackSnapshot>("clear_queue");
      onSnapshotChange(next);
    } catch (e) {
      onError(normalizeAppError(e));
    }
  };

  const handleMoveQueueItem = async (queueItemId: string, targetIndex: number) => {
    try {
      const next = await callCommand<PlaybackSnapshot>("move_in_queue", {
        queueItemId,
        targetIndex,
      });
      onSnapshotChange(next);
    } catch (e) {
      onError(normalizeAppError(e));
    }
  };

  const handleActivateSavedLoop = async (loop: AbLoopDto) => {
    try {
      const next = await callCommand<PlaybackSnapshot>("set_ab_loop", {
        startMs: loop.startMs,
        endMs: loop.endMs,
        label: loop.label,
        saveToDb: false,
      });
      onSnapshotChange(next);
    } catch (err) {
      onError(normalizeAppError(err));
    }
  };

  const handleApplySleepMinutes = async (customFadeSec?: number) => {
    const fadeSec = customFadeSec ?? sleepFadeOutSec;
    setSleepFadeOutSec(fadeSec);
    try {
      const next = await callCommand<PlaybackSnapshot>("set_sleep_timer", {
        mode: "minutes",
        minutes: sleepMinutesInput,
        fadeOutSeconds: fadeSec,
      });
      onSnapshotChange(next);
    } catch (err) {
      onError(normalizeAppError(err));
    }
  };

  const handleSelectSubtitle = async (attId: string, offset: number) => {
    setSelectedSubId(attId);
    setSubtitleOffsetMs(offset);
    if (!attId) {
      setCues([]);
      onSubtitlesChanged?.();
      return;
    }
    try {
      await callCommand("bind_subtitle_to_version", {
        attachmentId: attId,
        versionId: snapshot.currentVersionId ?? null,
        offsetMs: offset,
      });
      const loaded = await callCommand<SubtitleCueDto[]>("load_subtitle_cues", {
        attachmentId: attId,
      });
      setCues(loaded);
      onSubtitlesChanged?.();
    } catch (err) {
      onError(normalizeAppError(err));
    }
  };

  const handleFetchCloudSubtitles = async () => {
    if (!snapshot.currentWorkId) return;
    setFetchingCloudSubs(true);
    try {
      await callCommand<WorkDetailDto>("fetch_online_subtitles_for_work", {
        workId: snapshot.currentWorkId,
        rjOrId: null,
      });
      await loadWorkSubtitles();
      onSubtitlesChanged?.();
    } catch (err) {
      onError(normalizeAppError(err));
    } finally {
      setFetchingCloudSubs(false);
    }
  };

  const activeCueIndex = cues.findIndex(
    (c) => snapshot.positionMs >= c.startMs && snapshot.positionMs <= c.endMs
  );

  const filteredCues = subtitleKeyword.trim()
    ? cues.filter((c) =>
        c.text.toLowerCase().includes(subtitleKeyword.trim().toLowerCase())
      )
    : cues;
  const cueWindows = subtitleWindows(filteredCues.length, visibleCueLimit, subtitleKeyword.trim() ? -1 : activeCueIndex);
  const visibleCues = cueWindows.flatMap(range => filteredCues.slice(range.start, range.end));

  const syncLocalDspToSnapshot = (
    nextCfg: DspConfigDto,
    nextPresetId: string | null
  ) => {
    setDspConfig(nextCfg);
    setActivePresetId(nextPresetId);
    const summaryParts: string[] = [];
    if (nextCfg.masterBypass) {
      summaryParts.push("原声直通 (Bit-Perfect)");
    } else {
      if (nextCfg.hpfEnabled) summaryParts.push(`HPF ${Math.round(nextCfg.hpfCutoffHz)}Hz`);
      if (nextCfg.autoeqEnabled && nextCfg.autoeqBands.length > 0) {
        summaryParts.push(`AutoEQ(${nextCfg.autoeqBands.length}段)`);
      }
      if (nextCfg.deEsserEnabled) {
        summaryParts.push(`去齿音 ${nextCfg.deEsserGainDb.toFixed(1)}dB`);
      }
      if (nextCfg.airEnabled) {
        summaryParts.push(`空气感 +${nextCfg.airGainDb.toFixed(1)}dB`);
      }
      if (nextCfg.swapChannels) {
        summaryParts.push("L/R互换");
      }
      if (nextCfg.bs2bEnabled) {
        summaryParts.push("BS2B");
      }
      if (nextCfg.limiterEnabled) {
        summaryParts.push(`防炸耳 ${nextCfg.limiterThresholdDb.toFixed(1)}dB`);
      }
    }
    onSnapshotChange({
      ...snapshot,
      eventSeq: snapshot.eventSeq + 1,
      dspConfig: nextCfg,
      activeDspPresetId: nextPresetId,
      audioTelemetry: {
        ...snapshot.audioTelemetry,
        activeDspSummary: summaryParts.join(" · ") || "原声直通",
        activeFilterGraph: nextCfg.masterBypass ? "" : snapshot.audioTelemetry.activeFilterGraph,
      },
    });
  };

  const handleUpdateDspConfig = async (
    nextCfg: DspConfigDto,
    presetId: string | null = null
  ) => {
    syncLocalDspToSnapshot(nextCfg, presetId);
    try {
      const res = await callCommand<DspStateDto>("update_dsp_config", {
        config: nextCfg,
        presetId,
      });
      setDspConfig(res.config);
      setActivePresetId(res.activePresetId ?? null);
    } catch (e) {
      const err = normalizeAppError(e);
      if (err.code !== "IPC_UNAVAILABLE") onError(err);
    }
  };

  const handleApplyPreset = async (preset: DspPresetDto) => {
    syncLocalDspToSnapshot({ ...preset.config }, preset.id);
    try {
      const res = await callCommand<DspStateDto>("apply_dsp_preset", {
        presetId: preset.id,
      });
      setDspConfig(res.config);
      setActivePresetId(res.activePresetId ?? null);
    } catch (e) {
      const err = normalizeAppError(e);
      if (err.code !== "IPC_UNAVAILABLE") onError(err);
    }
  };

  const handleToggleBypass = async () => {
    const nextBypass = !dspConfig.masterBypass;
    const nextCfg: DspConfigDto = {
      ...dspConfig,
      masterBypass: nextBypass,
    };
    await handleUpdateDspConfig(nextCfg, nextBypass ? "bypass" : activePresetId);
  };

  const handleToggleAudioExclusive = async () => {
    const nextEx = !snapshot.audioExclusive;
    onSnapshotChange({
      ...snapshot,
      eventSeq: snapshot.eventSeq + 1,
      audioExclusive: nextEx,
    });
    try {
      const res = await callCommand<PlaybackSnapshot>("set_audio_exclusive", {
        exclusive: nextEx,
      });
      onSnapshotChange(res);
    } catch (e) {
      const err = normalizeAppError(e);
      if (err.code !== "IPC_UNAVAILABLE") onError(err);
    }
  };

  const handleInit5BandPeq = async () => {
    const nextCfg: DspConfigDto = {
      ...dspConfig,
      masterBypass: false,
      autoeqEnabled: true,
      autoeqName: "交互式 5 段 PEQ",
      autoeqBands: [
        { enabled: true, filterType: "LSC", frequencyHz: 95, gainDb: 1.5, qFactor: 0.71 },
        { enabled: true, filterType: "PK", frequencyHz: 380, gainDb: -1.0, qFactor: 1.4 },
        { enabled: true, filterType: "PK", frequencyHz: 1500, gainDb: 1.2, qFactor: 1.6 },
        { enabled: true, filterType: "PK", frequencyHz: 5200, gainDb: -2.0, qFactor: 2.8 },
        { enabled: true, filterType: "HSC", frequencyHz: 11500, gainDb: 2.0, qFactor: 0.71 },
      ],
    };
    await handleUpdateDspConfig(nextCfg, null);
  };

  const handleImportAutoEq = async () => {
    if (!autoEqTextInput.trim()) return;
    try {
      const res = await callCommand<DspStateDto>("import_autoeq_text", {
        text: autoEqTextInput,
      });
      setDspConfig(res.config);
      setActivePresetId(res.activePresetId ?? null);
      setShowAutoEqImport(false);
    } catch (e) {
      const err = normalizeAppError(e);
      if (err.code === "IPC_UNAVAILABLE") {
        // Parse basic Filter lines locally in non-Tauri test environment
        const nextCfg: DspConfigDto = {
          ...dspConfig,
          masterBypass: false,
          autoeqEnabled: true,
          autoeqPreampDb: -4.2,
          autoeqBands: [
            {
              enabled: true,
              filterType: "PK",
              frequencyHz: 3200,
              gainDb: -2.8,
              qFactor: 2.1,
            },
          ],
        };
        syncLocalDspToSnapshot(nextCfg, null);
        setShowAutoEqImport(false);
      } else {
        onError(err);
      }
    }
  };

  const handleClearAutoEq = async () => {
    const clearedCfg: DspConfigDto = {
      ...dspConfig,
      autoeqEnabled: false,
      autoeqName: null,
      autoeqBands: [],
      autoeqPreampDb: 0,
    };
    syncLocalDspToSnapshot(clearedCfg, null);
    try {
      const res = await callCommand<DspStateDto>("clear_autoeq_bands");
      setDspConfig(res.config);
    } catch (e) {
      const err = normalizeAppError(e);
      if (err.code !== "IPC_UNAVAILABLE") onError(err);
    }
  };

  const handleSaveCustomPreset = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!customPresetName.trim()) return;
    const name = customPresetName.trim();
    setCustomPresetName("");
    try {
      const res = await callCommand<DspStateDto>("save_custom_dsp_preset", {
        name,
        description: "用户自定义声学与防炸耳预设",
        config: dspConfig,
      });
      setDspConfig(res.config);
      setActivePresetId(res.activePresetId ?? null);
      setDspPresets(res.presets);
    } catch (err) {
      const appErr = normalizeAppError(err);
      if (appErr.code !== "IPC_UNAVAILABLE") onError(appErr);
    }
  };

  const handleToggleWorkOverride = async (enabled: boolean) => {
    setWorkOverrideEnabled(enabled);
    if (!snapshot.currentWorkId) return;
    try {
      const res = await callCommand<DspStateDto>("set_work_dsp_override_enabled", {
        workId: snapshot.currentWorkId,
        enabled,
      });
      setDspConfig(res.config);
      setActivePresetId(res.activePresetId ?? null);
      setWorkOverrideEnabled(res.workOverrideEnabled);
    } catch (e) {
      const err = normalizeAppError(e);
      if (err.code !== "IPC_UNAVAILABLE") onError(err);
    }
  };

  // Build SVG path & interactive coordinate mapping for 20Hz-20kHz frequency response curve (width=320, height=104, range -12dB..+6dB)
  const svgWidth = 320;
  const svgHeight = 104;
  const dbToY = (db: number) => {
    const clamped = Math.max(-12, Math.min(6, db));
    return 10 + ((6 - clamped) / 18) * 84;
  };
  const yToDb = (y: number) => {
    const ratio = (y - 10) / 84;
    const db = 6 - ratio * 18;
    return Math.round(Math.max(-12, Math.min(6, db)) * 2) / 2;
  };
  const freqToX = (freqHz: number) => {
    const clamped = Math.max(20, Math.min(20000, freqHz));
    const t = Math.log10(clamped / 20) / 3;
    return t * svgWidth;
  };
  const xToFreq = (x: number) => {
    const t = Math.max(0, Math.min(1, x / svgWidth));
    return Math.round(20 * Math.pow(1000, t));
  };
  const zeroDbY = dbToY(0);
  const curvePoints = freqCurveDb.map((db, idx) => {
    const x = (idx / Math.max(1, freqCurveDb.length - 1)) * svgWidth;
    const y = dbToY(db);
    return `${x.toFixed(1)},${y.toFixed(1)}`;
  });
  const polylinePoints = curvePoints.join(" ");
  const areaPoints = `0,${zeroDbY.toFixed(1)} ${polylinePoints} ${svgWidth},${zeroDbY.toFixed(1)}`;

  const handleSvgPointerMove = (e: React.PointerEvent<SVGSVGElement>) => {
    if (draggingBandIdx === null) return;
    const rect = e.currentTarget.getBoundingClientRect();
    const relX = ((e.clientX - rect.left) / Math.max(1, rect.width)) * svgWidth;
    const relY = ((e.clientY - rect.top) / Math.max(1, rect.height)) * svgHeight;
    const newFreq = xToFreq(relX);
    const newGain = yToDb(relY);
    const nextBands = dspConfig.autoeqBands.map((b, i) =>
      i === draggingBandIdx
        ? { ...b, enabled: true, frequencyHz: newFreq, gainDb: newGain }
        : b
    );
    const nextCfg: DspConfigDto = {
      ...dspConfig,
      masterBypass: false,
      autoeqEnabled: true,
      autoeqBands: nextBands,
    };
    syncLocalDspToSnapshot(nextCfg, null);
  };

  const handleSvgPointerUp = () => {
    if (draggingBandIdx !== null) {
      const idx = draggingBandIdx;
      setDraggingBandIdx(null);
      if (dspConfig.autoeqBands[idx]) {
        void handleUpdateDspConfig(dspConfig, null);
      }
    }
  };

  const handleBandWheel = (idx: number, e: React.WheelEvent) => {
    e.preventDefault();
    const delta = e.deltaY < 0 ? 0.25 : -0.25;
    const nextBands = dspConfig.autoeqBands.map((b, i) =>
      i === idx
        ? {
            ...b,
            qFactor: Math.round(Math.max(0.3, Math.min(10.0, b.qFactor + delta)) * 10) / 10,
          }
        : b
    );
    void handleUpdateDspConfig(
      {
        ...dspConfig,
        masterBypass: false,
        autoeqEnabled: true,
        autoeqBands: nextBands,
      },
      null
    );
  };

  return (
    <aside className="right-drawer player-glass-panel" aria-label="右侧辅助抽屉">
      <div className="drawer-header">
        <h3 className="drawer-header-title">
          {activeTab === "queue"
            ? `播放队列 (${snapshot.queue.length})`
            : activeTab === "bookmarks"
            ? "书签 · 循环 · 睡眠定时"
            : activeTab === "dsp"
            ? "专业音效与防炸耳 (DSP / EQ)"
            : activeTab === "audio"
            ? "WASAPI 独占"
            : "字幕台本"}
        </h3>
        <div className="drawer-header-actions">
          {activeTab === "dsp" && (
            <button
              type="button"
              className={`dsp-scope-toggle-btn btn-bypass-toggle ${
                dspConfig.masterBypass ? "is-lit is-bypassed" : "is-dim"
              }`}
              onClick={() => void handleToggleBypass()}
              aria-pressed={dspConfig.masterBypass}
              title="一键切换原声直通对比 (A/B Bypass)，验证音色与防炸耳差异"
            >
              <span>
                {dspConfig.masterBypass ? "直通中 (点击恢复)" : "直通对比 (Bypass)"}
              </span>
            </button>
          )}
          {activeTab === "queue" && snapshot.queue.length > 0 && (
            <button
              type="button"
              className="btn btn-ghost btn-sm"
              onClick={() => void handleClearQueue()}
              title="清空播放队列（不会删除任何本地音频文件）"
            >
              <span>清空队列</span>
            </button>
          )}
          <button
            type="button"
            className="btn-icon drawer-close-btn"
            onClick={onClose}
            aria-label="关闭抽屉"
            title="关闭抽屉"
          >
            <X size={16} />
          </button>
        </div>
      </div>

      {activeTab === "audio" && (
        <AudioOutputPanel snapshot={snapshot} onSnapshotChange={onSnapshotChange} onError={onError} />
      )}

      {activeTab === "queue" && (
        <div className="drawer-body">
          <QueuePlans snapshot={snapshot} onSnapshotChange={onSnapshotChange} />
          {snapshot.queue.length === 0 ? (
            <div className="empty-box">
              当前播放队列为空。可在作品详情页点击「全部加入队列」或选择单条音轨入队。
            </div>
          ) : (
            <>
              <ul className="queue-list">
                {snapshot.queue.slice(0, visibleQueueLimit).map((item, idx) => {
                  const isCurrent = item.id === snapshot.currentQueueItemId;
                  return (
                    <li
                      key={item.id}
                      className={`queue-item ${isCurrent ? "current" : ""}`}
                    >
                      <div className="queue-item-main">
                        <button
                          type="button"
                          className="queue-play-btn"
                          onClick={() =>
                            void handlePlayQueueItem(
                              item.versionId,
                              item.preferredSourceId
                            )
                          }
                        >
                          <Play size={13} />
                          <span className="queue-track-title">{item.trackTitle}</span>
                        </button>
                        <div className="queue-meta">
                          <span>{item.workTitle}</span>
                          {item.logicalDirPath && <span>· {item.logicalDirPath}</span>}
                          {item.positionMs > 0 && !item.completed && (
                            <span>· 续播 {formatDurationMs(item.positionMs)}</span>
                          )}
                          {item.completed && <span className="text-accent">· 已听完</span>}
                        </div>
                      </div>
                      <div className="queue-item-actions">
                        <button
                          type="button"
                          className="btn-icon-sm"
                          disabled={idx === 0}
                          onClick={() => void handleMoveQueueItem(item.id, idx - 1)}
                          aria-label="上移"
                          title="上移"
                        >
                          <ArrowUp size={13} />
                        </button>
                        <button
                          type="button"
                          className="btn-icon-sm"
                          disabled={idx === snapshot.queue.length - 1}
                          onClick={() => void handleMoveQueueItem(item.id, idx + 1)}
                          aria-label="下移"
                          title="下移"
                        >
                          <ArrowDown size={13} />
                        </button>
                        <button
                          type="button"
                          className="btn-icon-sm"
                          onClick={() => void handleRemoveQueueItem(item.id)}
                          aria-label="移出队列"
                          title="移出队列"
                        >
                          <Trash2 size={13} />
                        </button>
                      </div>
                    </li>
                  );
                })}
              </ul>
              {snapshot.queue.length > visibleQueueLimit && (
                <div className="pagination-row">
                  <button
                    type="button"
                    className="btn btn-secondary btn-sm"
                    onClick={() => setVisibleQueueLimit((n) => n + 100)}
                  >
                    展开更多队列项 ({visibleQueueLimit} / {snapshot.queue.length})
                  </button>
                </div>
              )}
            </>
          )}
        </div>
      )}

      {activeTab === "bookmarks" && (
        <div className="drawer-body">
          <div className="drawer-section">
            <h4>
              <Clock size={14} />
              <span>睡眠定时与等响度渐弱淡出</span>
            </h4>
            <div className="sleep-controls">
              <input
                type="number"
                className="input-sm"
                min={1}
                max={480}
                value={sleepMinutesInput}
                onChange={(e) => setSleepMinutesInput(Number(e.target.value))}
              />
              <button
                type="button"
                className="btn btn-secondary btn-sm"
                onClick={() => void handleApplySleepMinutes()}
              >
                按分钟启动
              </button>
              <button
                type="button"
                className="btn btn-secondary btn-sm"
                onClick={() =>
                  void callCommand<PlaybackSnapshot>("set_sleep_timer", {
                    mode: "end_of_track",
                    fadeOutSeconds: sleepFadeOutSec,
                  }).then(onSnapshotChange)
                }
              >
                本轨结束暂停
              </button>
              {snapshot.sleepTimer.mode !== "off" && (
                <button
                  type="button"
                  className="btn btn-ghost btn-sm"
                  onClick={() =>
                    void callCommand<PlaybackSnapshot>("set_sleep_timer", {
                      mode: "off",
                    }).then(onSnapshotChange)
                  }
                >
                  取消定时
                </button>
              )}
            </div>
            <div className="sleep-fade-row">
              <span className="text-muted">渐弱淡出时长:</span>
              <div className="sleep-fade-pills">
                {[15, 30, 60, 0].map((sec) => (
                  <button
                    key={sec}
                    type="button"
                    className={`dsp-scope-toggle-btn ${
                      sleepFadeOutSec === sec ? "is-lit" : "is-dim"
                    }`}
                    aria-pressed={sleepFadeOutSec === sec}
                    onClick={() => {
                      setSleepFadeOutSec(sec);
                      if (snapshot.sleepTimer.mode === "minutes") {
                        void handleApplySleepMinutes(sec);
                      }
                    }}
                  >
                    <span>{sec === 0 ? "无淡出" : `${sec}秒渐弱`}</span>
                  </button>
                ))}
              </div>
            </div>
          </div>

          <BookmarkNotes snapshot={snapshot} onSnapshotChange={onSnapshotChange} onOpenWorkDetail={onOpenWorkDetail} />

          <div className="drawer-section">
            <h4>已保存的 A-B 循环区间</h4>
            {savedLoops.length === 0 ? (
              <div className="empty-box">
                可在底部播放栏点击「A-B 循环」依次设定 A 点与 B 点
              </div>
            ) : (
              <ul className="bookmark-list">
                {savedLoops.map((loop) => (
                  <li key={loop.id} className="bookmark-item">
                    <button
                      type="button"
                      className="bookmark-jump-btn"
                      onClick={() => void handleActivateSavedLoop(loop)}
                    >
                      <span className="bookmark-time">
                        {formatDurationMs(loop.startMs)} - {formatDurationMs(loop.endMs)}
                      </span>
                      <span className="bookmark-note">{loop.label}</span>
                    </button>
                    <button
                      type="button"
                      className="btn-icon-sm"
                      onClick={() =>
                        void callCommand("delete_ab_loop", { loopId: loop.id }).then(
                          loadBookmarksAndLoops
                        )
                      }
                    >
                      <Trash2 size={13} />
                    </button>
                  </li>
                ))}
              </ul>
            )}
          </div>
        </div>
      )}

      {activeTab === "subtitles" && (
        <div className="drawer-body">
          <div className="subtitle-toolbar">
            <div className="subtitle-select-row">
              <Select
                aria-label="字幕附件"
                value={selectedSubId}
                onValueChange={value => void handleSelectSubtitle(value, subtitleOffsetMs)}
                options={[{ value: "", label: "选择 LRC / SRT / VTT 字幕附件..." }, ...subtitles.map(s => ({ value: s.id, label: s.title }))]}
              />
              {snapshot.currentWorkId && (
                <button
                  type="button"
                  className="btn btn-secondary btn-sm"
                  disabled={fetchingCloudSubs}
                  onClick={() => void handleFetchCloudSubtitles()}
                  title="从 asmr.one 自动检索并配对同名字幕 (.lrc/.srt/.vtt)"
                >
                  <span>{fetchingCloudSubs ? "检索中..." : "抓取云端字幕"}</span>
                </button>
              )}
            </div>

            <div className="offset-row">
              <span>时间轴偏移: {subtitleOffsetMs} ms</span>
              <div className="offset-btns">
                <button
                  type="button"
                  className="btn btn-ghost btn-sm"
                  disabled={!selectedSubId}
                  onClick={() =>
                    void handleSelectSubtitle(selectedSubId, subtitleOffsetMs - 500)
                  }
                >
                  -500ms
                </button>
                <button
                  type="button"
                  className="btn btn-ghost btn-sm"
                  disabled={!selectedSubId}
                  onClick={() => void handleSelectSubtitle(selectedSubId, 0)}
                >
                  重置
                </button>
                <button
                  type="button"
                  className="btn btn-ghost btn-sm"
                  disabled={!selectedSubId}
                  onClick={() =>
                    void handleSelectSubtitle(selectedSubId, subtitleOffsetMs + 500)
                  }
                >
                  +500ms
                </button>
              </div>
            </div>

            {cues.length > 0 && (
              <input
                type="search"
                className="input-text subtitle-search-input"
                placeholder="搜索台词关键字定位时间轴..."
                value={subtitleKeyword}
                onChange={(e) => setSubtitleKeyword(e.target.value)}
              />
            )}
          </div>

          {cues.length === 0 ? (
            <div className="empty-box">
              当前未绑定或未找到本地 LRC / SRT / VTT 字幕文件，可点击上方「抓取云端字幕」自动匹配。
            </div>
          ) : (
            <>
              <ul className="cue-list">
                {visibleCues.map((cue, visibleIndex) => {
                    const isActive =
                      snapshot.positionMs >= cue.startMs &&
                      snapshot.positionMs <= cue.endMs;
                    return (
                      <React.Fragment key={cue.index}>
                        {cueWindows.length > 1 && visibleIndex === cueWindows[0]!.end && <li className="cue-gap">已折叠 {cueWindows[1]!.start - cueWindows[0]!.end} 行，可搜索定位或继续展开</li>}
                        <li
                        className={`cue-item ${isActive ? "active" : ""}`}
                      >
                        <button
                          type="button"
                          className="cue-btn"
                          onClick={() =>
                            void callCommand<PlaybackSnapshot>("seek_ms", {
                              positionMs: cue.startMs,
                            }).then(onSnapshotChange)
                          }
                        >
                          <span className="cue-time">{formatDurationMs(cue.startMs)}</span>
                          <span className="cue-text">{cue.text}</span>
                        </button>
                        </li>
                      </React.Fragment>
                    );
                  })}
              </ul>
              {filteredCues.length > visibleCues.length && (
                <div className="pagination-row">
                  <button
                    type="button"
                    className="btn btn-secondary btn-sm"
                    onClick={() => setVisibleCueLimit((n) => n + 200)}
                  >
                    展开更多字幕行 (
                    {visibleCues.length} / {filteredCues.length})
                  </button>
                </div>
              )}
            </>
          )}
        </div>
      )}

      {activeTab === "dsp" && (
        <div className="drawer-body dsp-drawer-body">
          {/* 1. 20Hz–20kHz 交互式可拖拽参数均衡器画布 (Interactive PEQ Canvas) */}
          <div className="dsp-curve-card">
            <div className="dsp-curve-header">
              <span className="dsp-curve-title">
                20 Hz – 20 kHz 交互式 PEQ 频响画布
              </span>
              <span className="dsp-curve-summary">
                {dspConfig.masterBypass
                  ? "原声直通 (0.0 dB)"
                  : snapshot.audioTelemetry?.activeDspSummary || "已启用声学保护"}
              </span>
            </div>
            <svg
              className="dsp-curve-svg dsp-interactive-peq"
              viewBox={`0 0 ${svgWidth} ${svgHeight}`}
              role="img"
              aria-label="20 Hz 至 20 kHz 频率响应曲线"
              onPointerMove={handleSvgPointerMove}
              onPointerUp={handleSvgPointerUp}
              onPointerLeave={handleSvgPointerUp}
            >
              {/* 0 dB 参考基准线 */}
              <line
                x1={0}
                y1={zeroDbY}
                x2={svgWidth}
                y2={zeroDbY}
                className="dsp-curve-zero-line"
              />
              {/* 面积填充与合成曲线 */}
              <polygon points={areaPoints} className="dsp-curve-area" />
              <polyline points={polylinePoints} className="dsp-curve-line" />
              {/* 可拖拽 PEQ 控制节点 */}
              {!dspConfig.masterBypass &&
                dspConfig.autoeqEnabled &&
                dspConfig.autoeqBands.map((band, idx) => {
                  const cx = freqToX(band.frequencyHz);
                  const cy = dbToY(band.gainDb);
                  const isDragging = draggingBandIdx === idx;
                  return (
                    <g
                      key={idx}
                      className={`peq-node-group ${isDragging ? "is-dragging" : ""}`}
                      onPointerDown={(e) => {
                        e.stopPropagation();
                        setDraggingBandIdx(idx);
                      }}
                      onWheel={(e) => handleBandWheel(idx, e)}
                    >
                      <circle
                        cx={cx}
                        cy={cy}
                        r={isDragging ? 7 : 5}
                        className="peq-node-handle"
                      />
                      <title>
                        {`频段 #${idx + 1} (${band.filterType}): ${Math.round(
                          band.frequencyHz
                        )} Hz | ${band.gainDb >= 0 ? "+" : ""}${band.gainDb.toFixed(
                          1
                        )} dB | Q=${band.qFactor.toFixed(
                          2
                        )} (拖拽调节频率/增益，滚轮调节 Q 值)`}
                      </title>
                    </g>
                  );
                })}
            </svg>
            <div className="dsp-curve-axis">
              <span>20 Hz</span>
              <span>50 Hz (HPF)</span>
              <span>1 kHz</span>
              <span>6.5 kHz (齿音)</span>
              <span>12 kHz (空气)</span>
              <span>20 kHz</span>
            </div>
            <div className="dsp-peq-toolbar">
              <span className="text-muted">
                {dspConfig.autoeqBands.length > 0
                  ? "拖拽圆点调频/增益 · 滚轮调 Q 值"
                  : "可开启交互控制点或导入 AutoEQ"}
              </span>
              {dspConfig.autoeqBands.length === 0 && (
                <button
                  type="button"
                  className="btn btn-secondary btn-sm"
                  onClick={() => void handleInit5BandPeq()}
                >
                  <span>初始化 5 段交互 PEQ</span>
                </button>
              )}
            </div>
          </div>

          {/* 2. 声学预设切换与单作品独立覆盖开关 */}
          <div className="drawer-section">
            <div className="dsp-section-head">
              <h4>
                <Sparkles size={14} />
                <span>声学预设与作品记忆</span>
              </h4>
              <button
                type="button"
                className={`dsp-scope-toggle-btn ${
                  workOverrideEnabled ? "is-lit" : "is-dim"
                }`}
                onClick={() =>
                  void handleToggleWorkOverride(!workOverrideEnabled)
                }
                aria-pressed={workOverrideEnabled}
                title={
                  workOverrideEnabled
                    ? "已开启：当前音效参数仅对当前作品生效（点击切换为全局生效）"
                    : "未开启：当前音效参数全局生效（点击切换为仅对当前作品生效）"
                }
              >
                <span>仅对当前作品生效</span>
              </button>
            </div>

            <div className="dsp-preset-grid">
              {dspPresets.map((preset) => {
                const isSelected = activePresetId === preset.id;
                return (
                  <button
                    key={preset.id}
                    type="button"
                    className={`dsp-preset-btn ${isSelected ? "active" : ""}`}
                    onClick={() => void handleApplyPreset(preset)}
                    title={preset.description}
                  >
                    <span className="dsp-preset-name">{preset.name}</span>
                    <span className="dsp-preset-desc">{preset.description}</span>
                  </button>
                );
              })}
            </div>

            <form
              className="bookmark-form"
              onSubmit={(e) => void handleSaveCustomPreset(e)}
            >
              <input
                type="text"
                className="input-text"
                placeholder="输入名称保存当前参数为自定义预设..."
                value={customPresetName}
                onChange={(e) => setCustomPresetName(e.target.value)}
              />
              <button
                type="submit"
                className="btn btn-secondary btn-sm"
                disabled={!customPresetName.trim()}
              >
                <Plus size={14} />
                <span>保存预设</span>
              </button>
            </form>
          </div>

          {/* 3. 防炸耳软削波峰值限制器 (Anti-Ear-Blast Lookahead Soft Limiter) */}
          <div
            className={`drawer-section dsp-module-card ${
              dspConfig.limiterEnabled && !dspConfig.masterBypass
                ? "is-module-lit"
                : "is-module-dim"
            }`}
          >
            <div className="dsp-module-header">
              <button
                type="button"
                className={`dsp-scope-toggle-btn dsp-module-toggle-btn ${
                  dspConfig.limiterEnabled && !dspConfig.masterBypass
                    ? "is-lit"
                    : "is-dim"
                }`}
                aria-pressed={dspConfig.limiterEnabled}
                onClick={() =>
                  void handleUpdateDspConfig(
                    {
                      ...dspConfig,
                      masterBypass: false,
                      limiterEnabled: !dspConfig.limiterEnabled,
                    },
                    null
                  )
                }
                title={
                  dspConfig.limiterEnabled
                    ? "已开启防炸耳峰值限制器（点击关闭）"
                    : "已关闭防炸耳峰值限制器（点击开启）"
                }
              >
                <ShieldAlert size={14} />
                <span>防炸耳峰值限制器 (Peak Limiter)</span>
              </button>
              <button
                type="button"
                className="btn btn-ghost btn-sm"
                onClick={() =>
                  void handleUpdateDspConfig(
                    {
                      ...dspConfig,
                      limiterEnabled: true,
                      limiterThresholdDb: -1.0,
                      limiterLookaheadMs: 4.0,
                      limiterReleaseMs: 80.0,
                    },
                    null
                  )
                }
                title="重置防炸耳限制器为默认安全参数 (-1.0 dBFS / 4.0 ms)"
              >
                <RotateCcw size={12} />
                <span>重置</span>
              </button>
            </div>

            <div className="dsp-param-row">
              <div className="dsp-param-label">
                <span>峰值天花板阈值 (Ceiling)</span>
                <span className="dsp-param-val">
                  {dspConfig.limiterThresholdDb.toFixed(1)} dBFS
                </span>
              </div>
              <input
                type="range"
                className="dsp-slider"
                aria-label="峰值天花板阈值"
                min={-12.0}
                max={-0.5}
                step={0.5}
                disabled={!dspConfig.limiterEnabled}
                value={dspConfig.limiterThresholdDb}
                onChange={(e) =>
                  void handleUpdateDspConfig(
                    {
                      ...dspConfig,
                      masterBypass: false,
                      limiterThresholdDb: Number(e.target.value),
                    },
                    null
                  )
                }
              />
            </div>

            <div className="dsp-param-row">
              <div className="dsp-param-label">
                <span>前瞻拦截时间 (Lookahead)</span>
                <span className="dsp-param-val">
                  {dspConfig.limiterLookaheadMs.toFixed(1)} ms
                </span>
              </div>
              <input
                type="range"
                className="dsp-slider"
                aria-label="前瞻拦截时间"
                min={3.0}
                max={5.0}
                step={0.5}
                disabled={!dspConfig.limiterEnabled}
                value={dspConfig.limiterLookaheadMs}
                onChange={(e) =>
                  void handleUpdateDspConfig(
                    {
                      ...dspConfig,
                      masterBypass: false,
                      limiterLookaheadMs: Number(e.target.value),
                    },
                    null
                  )
                }
              />
            </div>
          </div>

          {/* 4. 超低频截断 (Sub-Bass Rumble Cut HPF) */}
          <div
            className={`drawer-section dsp-module-card ${
              dspConfig.hpfEnabled && !dspConfig.masterBypass
                ? "is-module-lit"
                : "is-module-dim"
            }`}
          >
            <div className="dsp-module-header">
              <button
                type="button"
                className={`dsp-scope-toggle-btn dsp-module-toggle-btn ${
                  dspConfig.hpfEnabled && !dspConfig.masterBypass
                    ? "is-lit"
                    : "is-dim"
                }`}
                aria-pressed={dspConfig.hpfEnabled}
                onClick={() =>
                  void handleUpdateDspConfig(
                    {
                      ...dspConfig,
                      masterBypass: false,
                      hpfEnabled: !dspConfig.hpfEnabled,
                    },
                    null
                  )
                }
                title={
                  dspConfig.hpfEnabled
                    ? "已开启超低频截断（点击关闭）"
                    : "已关闭超低频截断（点击开启）"
                }
              >
                <SlidersHorizontal size={14} />
                <span>超低频截断 (Low-Cut HPF)</span>
              </button>
              <button
                type="button"
                className="btn btn-ghost btn-sm"
                onClick={() =>
                  void handleUpdateDspConfig(
                    {
                      ...dspConfig,
                      hpfEnabled: true,
                      hpfCutoffHz: 50,
                    },
                    null
                  )
                }
                title="重置超低频截断为 50 Hz (12 dB/oct 巴特沃斯)"
              >
                <RotateCcw size={12} />
                <span>重置</span>
              </button>
            </div>

            <div className="dsp-param-row">
              <div className="dsp-param-label">
                <span>高通截止频率 (滤除麦架震动与喷麦)</span>
                <span className="dsp-param-val">
                  {Math.round(dspConfig.hpfCutoffHz)} Hz
                </span>
              </div>
              <input
                type="range"
                className="dsp-slider"
                aria-label="高通截止频率"
                min={20}
                max={100}
                step={5}
                disabled={!dspConfig.hpfEnabled}
                value={dspConfig.hpfCutoffHz}
                onChange={(e) =>
                  void handleUpdateDspConfig(
                    {
                      ...dspConfig,
                      masterBypass: false,
                      hpfCutoffHz: Number(e.target.value),
                    },
                    null
                  )
                }
              />
            </div>
          </div>

          {/* 5. 齿音与刺耳声压制 (5-8kHz Narrow-Q De-Esser) */}
          <div
            className={`drawer-section dsp-module-card ${
              dspConfig.deEsserEnabled && !dspConfig.masterBypass
                ? "is-module-lit"
                : "is-module-dim"
            }`}
          >
            <div className="dsp-module-header">
              <button
                type="button"
                className={`dsp-scope-toggle-btn dsp-module-toggle-btn ${
                  dspConfig.deEsserEnabled && !dspConfig.masterBypass
                    ? "is-lit"
                    : "is-dim"
                }`}
                aria-pressed={dspConfig.deEsserEnabled}
                onClick={() =>
                  void handleUpdateDspConfig(
                    {
                      ...dspConfig,
                      masterBypass: false,
                      deEsserEnabled: !dspConfig.deEsserEnabled,
                    },
                    null
                  )
                }
                title={
                  dspConfig.deEsserEnabled
                    ? "已开启齿音与刺耳压制（点击关闭）"
                    : "已关闭齿音与刺耳压制（点击开启）"
                }
              >
                <SlidersHorizontal size={14} />
                <span>齿音与刺耳压制 (De-Esser)</span>
              </button>
              <button
                type="button"
                className="btn btn-ghost btn-sm"
                onClick={() =>
                  void handleUpdateDspConfig(
                    {
                      ...dspConfig,
                      deEsserFreqHz: 6500,
                      deEsserQ: 4.0,
                      deEsserGainDb: -3.5,
                    },
                    null
                  )
                }
                title="重置齿音压制为 6500 Hz / Q 4.0 / -3.5 dB"
              >
                <RotateCcw size={12} />
                <span>重置</span>
              </button>
            </div>

            <div className="dsp-param-row">
              <div className="dsp-param-label">
                <span>中心频率 (5–8 kHz 口腔/掏耳敏感带)</span>
                <span className="dsp-param-val">
                  {Math.round(dspConfig.deEsserFreqHz)} Hz
                </span>
              </div>
              <input
                type="range"
                className="dsp-slider"
                aria-label="齿音压制中心频率"
                min={5000}
                max={8000}
                step={100}
                disabled={!dspConfig.deEsserEnabled}
                value={dspConfig.deEsserFreqHz}
                onChange={(e) =>
                  void handleUpdateDspConfig(
                    {
                      ...dspConfig,
                      masterBypass: false,
                      deEsserFreqHz: Number(e.target.value),
                    },
                    null
                  )
                }
              />
            </div>

            <div className="dsp-param-row">
              <div className="dsp-param-label">
                <span>衰减深度 (仅允许负向削减)</span>
                <span className="dsp-param-val">
                  {dspConfig.deEsserGainDb.toFixed(1)} dB (Q={dspConfig.deEsserQ.toFixed(1)})
                </span>
              </div>
              <input
                type="range"
                className="dsp-slider"
                aria-label="齿音压制衰减深度"
                min={-8.0}
                max={0.0}
                step={0.5}
                disabled={!dspConfig.deEsserEnabled}
                value={dspConfig.deEsserGainDb}
                onChange={(e) =>
                  void handleUpdateDspConfig(
                    {
                      ...dspConfig,
                      masterBypass: false,
                      deEsserGainDb: Number(e.target.value),
                    },
                    null
                  )
                }
              />
            </div>
          </div>

          {/* 6. 贴耳空气感增强 (10-14kHz High-Shelf Air Boost) */}
          <div
            className={`drawer-section dsp-module-card ${
              dspConfig.airEnabled && !dspConfig.masterBypass
                ? "is-module-lit"
                : "is-module-dim"
            }`}
          >
            <div className="dsp-module-header">
              <button
                type="button"
                className={`dsp-scope-toggle-btn dsp-module-toggle-btn ${
                  dspConfig.airEnabled && !dspConfig.masterBypass
                    ? "is-lit"
                    : "is-dim"
                }`}
                aria-pressed={dspConfig.airEnabled}
                onClick={() =>
                  void handleUpdateDspConfig(
                    {
                      ...dspConfig,
                      masterBypass: false,
                      airEnabled: !dspConfig.airEnabled,
                    },
                    null
                  )
                }
                title={
                  dspConfig.airEnabled
                    ? "已开启贴耳空气感增强（点击关闭）"
                    : "已关闭贴耳空气感增强（点击开启）"
                }
              >
                <Sparkles size={14} />
                <span>贴耳空气感 (Air Boost)</span>
              </button>
              <button
                type="button"
                className="btn btn-ghost btn-sm"
                onClick={() =>
                  void handleUpdateDspConfig(
                    {
                      ...dspConfig,
                      airFreqHz: 12000,
                      airGainDb: 1.5,
                    },
                    null
                  )
                }
                title="重置空气感增强为 12000 Hz / +1.5 dB"
              >
                <RotateCcw size={12} />
                <span>重置</span>
              </button>
            </div>

            <div className="dsp-param-row">
              <div className="dsp-param-label">
                <span>高频空气感增益 (12 kHz 高架提升)</span>
                <span className="dsp-param-val">
                  +{dspConfig.airGainDb.toFixed(1)} dB @ {(dspConfig.airFreqHz / 1000).toFixed(1)} kHz
                </span>
              </div>
              <input
                type="range"
                className="dsp-slider"
                aria-label="高频空气感增益"
                min={0.0}
                max={3.0}
                step={0.5}
                disabled={!dspConfig.airEnabled}
                value={dspConfig.airGainDb}
                onChange={(e) =>
                  void handleUpdateDspConfig(
                    {
                      ...dspConfig,
                      masterBypass: false,
                      airGainDb: Number(e.target.value),
                    },
                    null
                  )
                }
              />
            </div>
          </div>

          {/* 7. 前置增益、双耳空间微调与 WASAPI 独占直通 (Preamp, Binaural & WASAPI) */}
          <div className="drawer-section dsp-module-card">
            <div className="dsp-module-header">
              <span className="dsp-module-title">
                双耳空间微调与输出引擎 (Binaural & WASAPI)
              </span>
              <button
                type="button"
                className="btn btn-ghost btn-sm"
                onClick={() =>
                  void handleUpdateDspConfig(
                    {
                      ...dspConfig,
                      userPreampDb: 0,
                      balanceDb: 0,
                      swapChannels: false,
                      bs2bEnabled: false,
                    },
                    null
                  )
                }
                title="重置前置增益、左右耳平衡、声道互换与交叉馈送"
              >
                <RotateCcw size={12} />
                <span>重置</span>
              </button>
            </div>

            <div className="binaural-toggle-grid">
              <button
                type="button"
                className={`dsp-scope-toggle-btn ${
                  dspConfig.swapChannels && !dspConfig.masterBypass
                    ? "is-lit"
                    : "is-dim"
                }`}
                aria-pressed={Boolean(dspConfig.swapChannels)}
                onClick={() =>
                  void handleUpdateDspConfig(
                    {
                      ...dspConfig,
                      masterBypass: false,
                      swapChannels: !dspConfig.swapChannels,
                    },
                    null
                  )
                }
                title="一键互换左右声道 (pan=stereo|c0=c1|c1=c0)，纠正人头麦声道反接或侧卧反向收听"
              >
                <span>左右声道互换 (L⇄R)</span>
              </button>

              <button
                type="button"
                className={`dsp-scope-toggle-btn ${
                  dspConfig.bs2bEnabled && !dspConfig.masterBypass
                    ? "is-lit"
                    : "is-dim"
                }`}
                aria-pressed={Boolean(dspConfig.bs2bEnabled)}
                onClick={() =>
                  void handleUpdateDspConfig(
                    {
                      ...dspConfig,
                      masterBypass: false,
                      bs2bEnabled: !dspConfig.bs2bEnabled,
                    },
                    null
                  )
                }
                title="开启 Bauer 立体声转双耳交叉馈送 (700Hz / 4.5dB)，缓解长时间极端单侧耳音引起的听觉疲劳"
              >
                <span>双耳交叉馈送 (BS2B)</span>
              </button>

              <button
                type="button"
                className={`dsp-scope-toggle-btn ${
                  snapshot.audioExclusive ? "is-lit" : "is-dim"
                }`}
                aria-pressed={Boolean(snapshot.audioExclusive)}
                onClick={() => void handleToggleAudioExclusive()}
                title="启用 Windows WASAPI 独占直通模式 (--ao=wasapi --audio-exclusive=yes)，绕过系统重采样；若声卡被占用将自动安全回退"
              >
                <span>
                  {snapshot.audioExclusive
                    ? "WASAPI 独占直通 (已开启)"
                    : "WASAPI 共享模式 (点击独占)"}
                </span>
              </button>
            </div>

            <div className="dsp-param-row">
              <div className="dsp-param-label">
                <span>前置增益 (Preamp)</span>
                <span className="dsp-param-val">
                  {dspConfig.userPreampDb >= 0
                    ? `+${dspConfig.userPreampDb.toFixed(1)}`
                    : dspConfig.userPreampDb.toFixed(1)}{" "}
                  dB
                </span>
              </div>
              <input
                type="range"
                className="dsp-slider"
                aria-label="前置增益"
                min={-15.0}
                max={6.0}
                step={0.5}
                value={dspConfig.userPreampDb}
                onChange={(e) =>
                  void handleUpdateDspConfig(
                    {
                      ...dspConfig,
                      masterBypass: false,
                      userPreampDb: Number(e.target.value),
                    },
                    null
                  )
                }
              />
            </div>

            <div className="dsp-param-row">
              <div className="dsp-param-label">
                <span>左右耳平衡微调 (0.1 dB 步进)</span>
                <span className="dsp-param-val">
                  {Math.abs(dspConfig.balanceDb) < 0.05
                    ? "居中 0.0 dB"
                    : dspConfig.balanceDb < 0
                    ? `偏左 ${Math.abs(dspConfig.balanceDb).toFixed(1)} dB`
                    : `偏右 +${dspConfig.balanceDb.toFixed(1)} dB`}
                </span>
              </div>
              <input
                type="range"
                className="dsp-slider"
                aria-label="左右耳平衡补偿"
                min={-6.0}
                max={6.0}
                step={0.1}
                value={dspConfig.balanceDb}
                onChange={(e) =>
                  void handleUpdateDspConfig(
                    {
                      ...dspConfig,
                      masterBypass: false,
                      balanceDb: Number(Number(e.target.value).toFixed(1)),
                    },
                    null
                  )
                }
              />
            </div>
          </div>

          {/* 8. AutoEQ 耳机参数化校准 (ParametricEQ.txt 导入与管理) */}
          <div
            className={`drawer-section dsp-module-card ${
              dspConfig.autoeqEnabled && !dspConfig.masterBypass
                ? "is-module-lit"
                : "is-module-dim"
            }`}
          >
            <div className="dsp-module-header">
              <button
                type="button"
                className={`dsp-scope-toggle-btn dsp-module-toggle-btn ${
                  dspConfig.autoeqEnabled && !dspConfig.masterBypass
                    ? "is-lit"
                    : "is-dim"
                }`}
                aria-pressed={dspConfig.autoeqEnabled}
                onClick={() =>
                  void handleUpdateDspConfig(
                    {
                      ...dspConfig,
                      masterBypass: false,
                      autoeqEnabled: !dspConfig.autoeqEnabled,
                    },
                    null
                  )
                }
                title={
                  dspConfig.autoeqEnabled
                    ? "已开启 AutoEQ 耳机校准（点击关闭）"
                    : "已关闭 AutoEQ 耳机校准（点击开启）"
                }
              >
                <SlidersHorizontal size={14} />
                <span>
                  AutoEQ 耳机校准 ({dspConfig.autoeqBands.length} 段滤波器)
                </span>
              </button>
              <div className="track-actions">
                <button
                  type="button"
                  className="btn btn-secondary btn-sm"
                  onClick={() => setShowAutoEqImport((v) => !v)}
                >
                  <Upload size={12} />
                  <span>导入配置</span>
                </button>
                {dspConfig.autoeqBands.length > 0 && (
                  <button
                    type="button"
                    className="btn btn-ghost btn-sm"
                    onClick={() => void handleClearAutoEq()}
                  >
                    <span>清除校准</span>
                  </button>
                )}
              </div>
            </div>

            {showAutoEqImport && (
              <div className="autoeq-import-box">
                <textarea
                  className="input-textarea"
                  rows={4}
                  aria-label="AutoEQ ParametricEQ.txt 配置内容"
                  value={autoEqTextInput}
                  onChange={(e) => setAutoEqTextInput(e.target.value)}
                  placeholder="粘贴 EqualizerAPO / AutoEQ ParametricEQ.txt 内容..."
                />
                <div className="track-actions">
                  <button
                    type="button"
                    className="btn btn-primary btn-sm"
                    onClick={() => void handleImportAutoEq()}
                  >
                    解析并应用 AutoEQ
                  </button>
                  <button
                    type="button"
                    className="btn btn-ghost btn-sm"
                    onClick={() => setShowAutoEqImport(false)}
                  >
                    取消
                  </button>
                </div>
              </div>
            )}

            {dspConfig.autoeqBands.length > 0 && (
              <div className="autoeq-bands-summary">
                <span>
                  校准前置增益: {dspConfig.autoeqPreampDb.toFixed(1)} dB
                </span>
                <div className="autoeq-band-chips">
                  {dspConfig.autoeqBands.map((b, i) => (
                    <span key={i} className="badge badge-tech">
                      {b.filterType} {Math.round(b.frequencyHz)}Hz{" "}
                      {b.gainDb >= 0 ? `+${b.gainDb.toFixed(1)}` : b.gainDb.toFixed(1)}dB Q
                      {b.qFactor.toFixed(1)}
                    </span>
                  ))}
                </div>
              </div>
            )}
          </div>
        </div>
      )}
    </aside>
  );
};

