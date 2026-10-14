import { Select } from "../../components/Select";
import React, { useEffect, useState, useCallback, useRef } from "react";
import {
  ArrowLeft,
  Play,
  Plus,
  Download,
  Bookmark,
  Heart,
  RefreshCw,
  Edit3,
  FolderTree,
  HardDrive,
  Radio,
  CheckCircle2,
  Link2,
  RotateCcw,
  ChevronDown,
  ChevronRight,
} from "lucide-react";
import {
  WorkDetailDto,
  DirectoryGroupOption,
  TrackItemDto,
  DownloadPreviewDto,
  DownloadTaskDto,
  PlaybackSnapshot,
  AssociationHistoryDto,
  MediaAffinity,
  subscribeLibraryAffinity,
  callCommand,
  formatDurationMs,
  normalizeAppError,
  AppError,
} from "../../lib/api";
import { DownloadTaskModal } from "../../components/DownloadTaskModal";
import { CopyRjBadge } from "./CopyRjBadge";

function isNoSeText(text: string): boolean {
  const lower = text.toLowerCase();
  return [
    "seなし",
    "se無し",
    "seナシ",
    "无音效",
    "無効果音",
    "効果音なし",
    "効果音無し",
    "nose",
    "no_se",
    "no-se",
    "without se",
    "without_se",
    "なし版",
    "無し版",
  ].some((kw) => lower.includes(kw));
}

function hasPositiveSeText(text: string): boolean {
  const lower = text.toLowerCase();
  return [
    "seあり",
    "se有り",
    "seアリ",
    "有音效",
    "効果音あり",
    "効果音有り",
    "with se",
    "with_se",
    "あり版",
    "有り版",
  ].some((kw) => lower.includes(kw));
}

function isBonusGroupText(text: string): boolean {
  const lower = text.toLowerCase();
  return [
    "特典",
    "おまけ",
    "ボーナストラック",
    "bonus",
    "extra",
    "フリートーク",
    "free talk",
    "試聴",
    "体験版",
    "予告",
    "trailer",
  ].some((kw) => lower.includes(kw));
}

function scoreGroupFormat(group: DirectoryGroupOption): number {
  const upperFormats = group.formats.map((f) => f.trim().toUpperCase());
  const pathLower = `${group.groupPath} ${group.label}`.toLowerCase();
  if (upperFormats.includes("WAV") || /\bwav\b/.test(pathLower)) {
    return 3;
  }
  if (upperFormats.includes("FLAC") || /\bflac\b/.test(pathLower)) {
    return 2;
  }
  if (
    upperFormats.some((f) =>
      ["MP3", "M4A", "OGG", "OPUS", "AAC"].includes(f)
    ) ||
    /\b(mp3|m4a|ogg|opus|aac)\b/.test(pathLower)
  ) {
    return 1;
  }
  return 0;
}

/**
 * Selects the best default directory group for a work:
 * 1. With sound effects (`有音效` / `SEあり`, excluding `SEなし` / `无音效`)
 * 2. WAV format (`WAV > FLAC > MP3/M4A/OGG/OPUS`)
 * 3. Non-bonus main tracks (`!isBonus`)
 * 4. Maximum track count (`音轨最多`)
 * 5. Explicit positive SE marker (`SEあり` / `有音效`)
 */
export function pickBestDefaultDirectoryGroup(
  groups: DirectoryGroupOption[],
  tracks?: TrackItemDto[]
): string {
  if (groups.length === 0) return "";

  const scoreTuple = (g: DirectoryGroupOption): number[] => {
    const groupText = `${g.groupPath} ${g.label}`;
    const gTracks = tracks
      ? tracks.filter((t) => t.logicalDirPath === g.groupPath)
      : [];
    const noSe =
      isNoSeText(groupText) ||
      (gTracks.length > 0 &&
        gTracks.every(
          (t) => isNoSeText(t.title) || isNoSeText(t.groupLabel)
        ));
    const posSe =
      hasPositiveSeText(groupText) ||
      gTracks.some(
        (t) => hasPositiveSeText(t.title) || hasPositiveSeText(t.groupLabel)
      );
    const isBonus = isBonusGroupText(groupText);
    const fmt = scoreGroupFormat(g);

    return [
      noSe ? 0 : 1,
      fmt,
      isBonus ? 0 : 1,
      g.trackCount,
      posSe ? 1 : 0,
    ];
  };

  let best = groups[0]!;
  let bestScore = scoreTuple(best);

  for (let i = 1; i < groups.length; i++) {
    const candidate = groups[i]!;
    const candScore = scoreTuple(candidate);
    for (let k = 0; k < candScore.length; k++) {
      if (candScore[k]! > bestScore[k]!) {
        best = candidate;
        bestScore = candScore;
        break;
      }
      if (candScore[k]! < bestScore[k]!) {
        break;
      }
    }
  }

  return best.groupPath;
}

interface WorkDetailViewProps {
  focusVersionId?: string;
  workId: string;
  onBack: () => void;
  onSnapshotChange: (snap: PlaybackSnapshot) => void;
  onOpenDownloads: () => void;
  onSearchKeyword?: (keyword: string, preferOnline?: boolean) => void;
  onError: (err: AppError) => void;
}

export const WorkDetailView: React.FC<WorkDetailViewProps> = ({
  workId,
  focusVersionId,
  onBack,
  onSnapshotChange,
  onOpenDownloads,
  onSearchKeyword,
  onError,
}) => {
  const [detail, setDetail] = useState<WorkDetailDto | null>(null);
  const [loading, setLoading] = useState(false);
  const [selectedGroupPath, setSelectedGroupPath] = useState<string>("");
  const [startingPlayback, setStartingPlayback] = useState(false);
  const [isDirCollapsed, setIsDirCollapsed] = useState<boolean>(true);
  const [visibleTrackLimit, setVisibleTrackLimit] = useState<number>(100);
  const lastInitWorkIdRef = useRef<string | null>(null);

  // Override modal state
  const [editingOverrides, setEditingOverrides] = useState(false);
  const [ovTitle, setOvTitle] = useState("");
  const [ovCircle, setOvCircle] = useState("");
  const [ovDesc, setOvDesc] = useState("");
  const [ovVas, setOvVas] = useState("");
  const [ovTags, setOvTags] = useState("");

  // Sync metadata & cloud subtitles from asmr.one state
  const [syncRjInput, setSyncRjInput] = useState("");
  const [syncingMeta, setSyncingMeta] = useState(false);
  const [fetchingSubs, setFetchingSubs] = useState(false);

  const [favoritePending, setFavoritePending] = useState<string | null>(null);
  const favoriteGate = useRef(false);

  // Download preview modal state (web asmr.one style file tree selector)
  const [dlPreview, setDlPreview] = useState<DownloadPreviewDto | null>(null);
  const [dlSelectedIds, setDlSelectedIds] = useState<Record<string, boolean>>({});
  const [preparingDownload, setPreparingDownload] = useState(false);

  // Association & Undo panel
  const [showAssocPanel, setShowAssocPanel] = useState(false);
  const [assocOnlineInput, setAssocOnlineInput] = useState("");
  const [mergeTargetVer, setMergeTargetVer] = useState("");
  const [mergeSourceVer, setMergeSourceVer] = useState("");
  const [assocHistory, setAssocHistory] = useState<AssociationHistoryDto[]>([]);

  const loadDetail = useCallback(async () => {
    setLoading(true);
    try {
      const d = await callCommand<WorkDetailDto>("get_work_detail", { workId });
      setDetail(d);
      setOvTitle(d.title);
      setOvCircle(d.circleName);
      setOvDesc(d.description);
      setOvVas(d.voiceActors.join(", "));
      setOvTags(d.tags.join(", "));
      if (d.rjCode) {
        setSyncRjInput(d.rjCode);
      }
      if (d.directoryGroups.length > 0) {
        const bestGroup = pickBestDefaultDirectoryGroup(
          d.directoryGroups,
          d.tracks
        );
        if (lastInitWorkIdRef.current !== workId) {
          lastInitWorkIdRef.current = workId;
          setSelectedGroupPath(bestGroup);
          setIsDirCollapsed(true);
        } else {
          setSelectedGroupPath((prev) =>
            d.directoryGroups.some((g) => g.groupPath === prev)
              ? prev
              : bestGroup
          );
        }
      } else {
        lastInitWorkIdRef.current = workId;
        setSelectedGroupPath("");
        setIsDirCollapsed(true);
      }
    } catch (e) {
      onError(normalizeAppError(e));
    } finally {
      setLoading(false);
    }
  }, [workId, onError]);

  useEffect(() => {
    void loadDetail();
  }, [loadDetail]);
  useEffect(() => {
    let disposed = false; let stop: (() => void) | undefined;
    let revision = 0;
    void subscribeLibraryAffinity(id => {
      if (disposed || id !== workId) return;
      const request = ++revision;
      void callCommand<WorkDetailDto>("get_work_detail", { workId }).then(next => {
        if (!disposed && request === revision) setDetail(prev => prev && next && prev.id === next.id ? { ...prev, isFavorite: next.isFavorite, isCollected: next.isCollected, tracks: next.tracks } : prev);
      }).catch(e => { if (!disposed) onError(normalizeAppError(e)); });
    }).then(fn => { if (disposed) fn(); else stop = fn; });
    return () => { disposed = true; stop?.(); };
  }, [workId, onError]);

  useEffect(() => {
    if (!focusVersionId || detail?.id !== workId) return;
    const track = detail.tracks.find(t => t.versions.some(v => v.id === focusVersionId));
    if (!track) return;
    setSelectedGroupPath(track.logicalDirPath);
    setVisibleTrackLimit(detail.tracks.length);
    const frame = requestAnimationFrame(() => {
      const row = document.getElementById(`bookmark-version-${focusVersionId}`);
      row?.scrollIntoView?.({ block: "center", behavior: "auto" });
      row?.focus({ preventScroll: true });
    });
    return () => cancelAnimationFrame(frame);
  }, [detail, focusVersionId, workId]);

  const handlePlayVersion = async (
    versionId: string,
    preferredSourceId?: string | null
  ) => {
    try {
      const snap = await callCommand<PlaybackSnapshot>("play_version", {
        versionId,
        preferredSourceId: preferredSourceId ?? null,
      });
      onSnapshotChange(snap);
    } catch (e) {
      onError(normalizeAppError(e));
    }
  };

  const handleAppendQueue = async (
    versionId: string,
    preferredSourceId?: string | null
  ) => {
    try {
      const snap = await callCommand<PlaybackSnapshot>("append_to_queue", {
        versionId,
        preferredSourceId: preferredSourceId ?? null,
      });
      onSnapshotChange(snap);
    } catch (e) {
      onError(normalizeAppError(e));
    }
  };

  const handlePlayGroup = async (groupPath: string, startVersionId?: string) => {
    try {
      const snap = await callCommand<PlaybackSnapshot>("play_directory_group", {
        workId,
        groupPath,
        startVersionId: startVersionId ?? null,
      });
      onSnapshotChange(snap);
    } catch (e) {
      onError(normalizeAppError(e));
    }
  };

  const handlePlayWork = async (fromStart: boolean) => {
    if (startingPlayback) return;
    setStartingPlayback(true);
    try {
      const snap = await callCommand<PlaybackSnapshot>("play_work", {
        workId, fromStart,
        groupPath: fromStart || !detail?.lastVersionId ? selectedGroupPath : null,
      });
      onSnapshotChange(snap);
    } catch (e) {
      onError(normalizeAppError(e));
    } finally {
      setStartingPlayback(false);
    }
  };

  const handleToggleCollected = async () => {
    try {
      const next = await callCommand<boolean>("toggle_collected", { workId });
      setDetail((prev) => (prev ? { ...prev, isCollected: next } : prev));
    } catch (e) {
      onError(normalizeAppError(e));
    }
  };

  const handleToggleFavorite = async () => {
    try {
      const next = await callCommand<boolean>("toggle_favorite", { workId });
      setDetail((prev) => (prev ? { ...prev, isFavorite: next } : prev));
    } catch (e) {
      onError(normalizeAppError(e));
    }
  };

  const handleSyncFromAsmrOne = async (preferOnlineCover: boolean) => {
    setSyncingMeta(true);
    try {
      const updated = await callCommand<WorkDetailDto>(
        "sync_work_from_asmr_one",
        {
          workId,
          rjOrId: syncRjInput.trim() || detail?.rjCode || null,
          preferOnlineCover,
        }
      );
      setDetail(updated);
      setOvTitle(updated.title);
      setOvCircle(updated.circleName);
      setOvDesc(updated.description);
      setOvVas(updated.voiceActors.join(", "));
      setOvTags(updated.tags.join(", "));
    } catch (e) {
      onError(normalizeAppError(e));
    } finally {
      setSyncingMeta(false);
    }
  };

  const handleFetchOnlineSubtitles = async () => {
    setFetchingSubs(true);
    try {
      const updated = await callCommand<WorkDetailDto>(
        "fetch_online_subtitles_for_work",
        {
          workId,
          rjOrId: syncRjInput.trim() || detail?.rjCode || null,
        }
      );
      setDetail(updated);
    } catch (e) {
      onError(normalizeAppError(e));
    } finally {
      setFetchingSubs(false);
    }
  };

  const handleFavoriteTrack = async (versionId: string) => {
    if (favoriteGate.current) return;
    favoriteGate.current = true; setFavoritePending(versionId);
    try {
      const next = await callCommand<MediaAffinity>("toggle_version_favorite", { versionId });
      setDetail(prev => prev && prev.id === next.workId ? { ...prev, isFavorite: next.isFavorite,
        tracks: prev.tracks.map(t => ({ ...t, versions: t.versions.map(v => v.id === versionId ? { ...v, isFavorite: next.isVersionFavorite } : v) })) } : prev);
    } catch (e) { onError(normalizeAppError(e)); }
    finally { favoriteGate.current = false; setFavoritePending(null); }
  };

  const handleSaveOverrideField = async (
    field: string,
    mode: "set" | "cleared",
    valueText: string | null
  ) => {
    try {
      const updated = await callCommand<WorkDetailDto>("set_work_override", {
        workId,
        field,
        overrideMode: mode,
        valueText,
      });
      setDetail(updated);
    } catch (e) {
      onError(normalizeAppError(e));
    }
  };

  const handleResetOverrideField = async (field: string) => {
    try {
      const updated = await callCommand<WorkDetailDto>("reset_work_override", {
        workId,
        field,
      });
      setDetail(updated);
    } catch (e) {
      onError(normalizeAppError(e));
    }
  };

  const handleOpenDownloadPreview = async (sourceIds?: string[]) => {
    setPreparingDownload(true);
    try {
      let prev = await callCommand<DownloadPreviewDto | null>(
        "prepare_work_download",
        {
          workId,
          rjOrId:
            detail?.rjCode ?? detail?.onlineSources[0]?.sourceWorkId ?? null,
        }
      );
      if (!prev || !Array.isArray(prev.items)) {
        prev = await callCommand<DownloadPreviewDto>("preview_download", {
          workId,
          sourceIds: sourceIds ?? null,
        });
      }
      setDlPreview(prev);
      const filterSet =
        sourceIds && sourceIds.length > 0 ? new Set(sourceIds) : null;
      const map: Record<string, boolean> = {};
      for (const item of prev.items) {
        const canSelect = !item.alreadyDownloaded && !item.alreadyQueued;
        map[item.sourceId] = filterSet
          ? canSelect && filterSet.has(item.sourceId)
          : canSelect;
      }
      setDlSelectedIds(map);
      await loadDetail();
    } catch (e) {
      onError(normalizeAppError(e));
    } finally {
      setPreparingDownload(false);
    }
  };

  const handleConfirmDownload = async () => {
    if (!dlPreview) return;
    const chosen = dlPreview.items
      .filter((i) => dlSelectedIds[i.sourceId])
      .map((i) => i.sourceId);
    if (chosen.length === 0) return;
    try {
      await callCommand<DownloadTaskDto[]>("enqueue_downloads", {
        workId,
        sourceIds: chosen,
      });
      setDlPreview(null);
      onOpenDownloads();
    } catch (e) {
      onError(normalizeAppError(e));
    }
  };

  const handleLoadAssocHistory = async () => {
    try {
      const hist = await callCommand<AssociationHistoryDto[]>(
        "list_association_history"
      );
      setAssocHistory(hist);
    } catch (e) {
      onError(normalizeAppError(e));
    }
  };

  const handleAssociateOnlineByInput = async () => {
    if (!assocOnlineInput.trim()) return;
    try {
      const onlineDetail = await callCommand<WorkDetailDto>("open_online_work", {
        workIdOrRj: assocOnlineInput.trim(),
      });
      if (onlineDetail.id !== workId) {
        await callCommand<string>("associate_works", {
          localWorkId: workId,
          onlineWorkId: onlineDetail.id,
        });
      }
      setAssocOnlineInput("");
      await loadDetail();
      await handleLoadAssocHistory();
    } catch (e) {
      onError(normalizeAppError(e));
    }
  };

  const handleMergeVersions = async () => {
    if (!mergeTargetVer || !mergeSourceVer || mergeTargetVer === mergeSourceVer)
      return;
    try {
      await callCommand<string>("associate_versions", {
        targetVersionId: mergeTargetVer,
        sourceVersionId: mergeSourceVer,
      });
      setMergeSourceVer("");
      await loadDetail();
      await handleLoadAssocHistory();
    } catch (e) {
      onError(normalizeAppError(e));
    }
  };

  const handleUndoAssociation = async (historyId: string) => {
    try {
      await callCommand("undo_association", { historyId });
      await loadDetail();
      await handleLoadAssocHistory();
    } catch (e) {
      onError(normalizeAppError(e));
    }
  };

  if (loading || !detail) {
    return (
      <div className="page-container">
        <div className="detail-top-bar">
          <button type="button" className="btn btn-ghost btn-sm" onClick={onBack}>
            <ArrowLeft size={16} />
            <span>返回</span>
          </button>
        </div>
      </div>
    );
  }

  const activeGroup = detail.directoryGroups.find(
    (g) => g.groupPath === selectedGroupPath
  );
  const filteredTracks = activeGroup
    ? detail.tracks.filter((t) => t.logicalDirPath === selectedGroupPath)
    : detail.tracks;

  // 本地导入作品或所有音轨均已落地本地时，不展示下载按钮
  const isLocalWork = detail.tracks.some((t) =>
    t.versions.some((v) =>
      v.sources.some((s) => s.sourceKind === "local_imported")
    )
  );

  const hasDownloadableOnline = detail.tracks.some((t) =>
    t.versions.some(
      (v) =>
        v.sources.some((s) => s.sourceKind === "online_provider") &&
        !v.sources.some(
          (s) =>
            (s.sourceKind === "local_imported" ||
              s.sourceKind === "local_downloaded") &&
            s.availability === "available"
        )
    )
  );

  const canDownloadOnline =
    !isLocalWork &&
    (hasDownloadableOnline ||
      (detail.tracks.length === 0 &&
        (detail.onlineSources.length > 0 || Boolean(detail.rjCode))));

  const allVersionsFlat = detail.tracks.flatMap((t) =>
    t.versions.map((v) => ({
      versionId: v.id,
      label: `[${t.logicalDirPath || "根"}] ${t.title} (${v.versionLabel}, 进度 ${formatDurationMs(v.positionMs)})`,
    }))
  );

  return (
    <div className="page-container">
      <div className="detail-top-bar">
        <button type="button" className="btn btn-ghost btn-sm" onClick={onBack}>
          <ArrowLeft size={16} />
          <span>返回</span>
        </button>

        <div className="detail-top-actions">
          {!detail.rjCode && (
            <input
              type="text"
              className="input-text input-sm"
              placeholder="RJ 号 (如 RJ01705068)"
              value={syncRjInput}
              onChange={(e) => setSyncRjInput(e.target.value)}
            />
          )}

          <button
            type="button"
            className="btn btn-ghost btn-sm"
            disabled={syncingMeta || (!detail.rjCode && !syncRjInput.trim())}
            onClick={() => void handleSyncFromAsmrOne(true)}
            title="从 asmr.one 同步封面、社团、CV 与标签"
          >
            <RefreshCw size={15} />
            <span>同步元数据</span>
          </button>

          <button
            type="button"
            className="btn btn-ghost btn-sm"
            disabled={fetchingSubs}
            onClick={() => void handleFetchOnlineSubtitles()}
            title="检索云端字幕或扫描本地字幕 (.lrc/.srt/.vtt)"
          >
            <RefreshCw size={15} />
            <span>抓取字幕 ({detail.attachments.length})</span>
          </button>

          <button
            type="button"
            className="btn btn-ghost btn-sm"
            onClick={() => setEditingOverrides((v) => !v)}
          >
            <Edit3 size={15} />
            <span>编辑元数据</span>
          </button>

          <button
            type="button"
            className="btn btn-ghost btn-sm"
            onClick={() => {
              setShowAssocPanel((v) => !v);
              void handleLoadAssocHistory();
            }}
          >
            <Link2 size={15} />
            <span>关联与撤销</span>
          </button>
        </div>
      </div>

      <section className="detail-hero-card">
        <div className="detail-hero-layout">
          {detail.coverRef && (
            <div className="detail-cover-wrap">
              <img
                src={detail.coverRef}
                alt={detail.title}
                className="detail-cover-img"
                referrerPolicy="no-referrer"
                onError={(e) => {
                  (e.currentTarget.parentElement as HTMLElement).style.display =
                    "none";
                }}
              />
            </div>
          )}

          <div className="detail-hero-main">
            {detail.rjCode && (
              <div className="work-badges">
                <CopyRjBadge key={detail.rjCode} code={detail.rjCode} />
              </div>
            )}

            <h2>{detail.title}</h2>
            {detail.description && (
              <p className="detail-desc">{detail.description}</p>
            )}

            <div className="work-card-meta">
              {detail.circleName && <span>社团: {detail.circleName}</span>}
              {detail.voiceActors.length > 0 && (
                <span className="cv-list">
                  CV:{" "}
                  {detail.voiceActors.map((va) => (
                    <button
                      key={va}
                      type="button"
                      className="cv-pill cv-pill-btn"
                      onClick={() => onSearchKeyword?.(va, !isLocalWork)}
                      aria-label={`搜索声优 ${va}`}
                      title={`点击寻找声优「${va}」的相关作品`}
                    >
                      {va}
                    </button>
                  ))}
                </span>
              )}
            </div>

            {detail.tags.length > 0 && (
              <div className="work-tags">
                {detail.tags.map((tag) => (
                  <button
                    key={tag}
                    type="button"
                    className="tag-pill tag-pill-btn"
                    onClick={() => onSearchKeyword?.(tag, !isLocalWork)}
                    aria-label={`搜索标签 ${tag}`}
                    title={`点击寻找包含标签「${tag}」的作品`}
                  >
                    {tag}
                  </button>
                ))}
              </div>
            )}

            <div className="detail-hero-buttons">
              {detail.lastVersionId ? (
                <button
                  type="button"
                  className="btn btn-primary"
                  disabled={startingPlayback}
                  onClick={() => void handlePlayWork(false)}
                >
                  <Play size={16} />
                  <span>
                    {detail.tracks.some(t => t.versions.some(v => v.id === detail.lastVersionId && v.completed))
                      ? "播放" : `继续收听 (${formatDurationMs(detail.lastPositionMs)})`}
                  </span>
                </button>
              ) : detail.directoryGroups.length > 0 ? (
                <button
                  type="button"
                  className="btn btn-primary"
                  disabled={startingPlayback}
                  onClick={() => void handlePlayWork(false)}
                >
                  <Play size={16} />
                  <span>播放</span>
                </button>
              ) : null}

              {detail.directoryGroups.length > 0 && <button type="button" className="btn btn-secondary"
                disabled={startingPlayback} title="从当前目录第一条音轨的开头播放"
                onClick={() => void handlePlayWork(true)}><RotateCcw size={16} />从头播放</button>}

              {!isLocalWork && <button
                type="button"
                className={detail.isCollected ? "btn btn-tonal" : "btn btn-secondary"}
                onClick={() => void handleToggleCollected()}
                aria-pressed={detail.isCollected}
                title={
                  detail.isCollected
                    ? "移出我的音频库（保留已下载文件）"
                    : "移到我的音频库"
                }
              >
                <Bookmark
                  size={16}
                  fill={detail.isCollected ? "currentColor" : "none"}
                />
                <span>{detail.isCollected ? "已在我的音频库" : "移到我的音频库"}</span>
              </button>}

              <button
                type="button"
                className={
                  detail.isFavorite
                    ? "btn btn-tonal text-accent"
                    : "btn btn-secondary"
                }
                onClick={() => void handleToggleFavorite()}
                aria-pressed={detail.isFavorite}
                title={
                  detail.isFavorite
                    ? "已喜欢（位于「喜欢」），点击取消"
                    : "加入「喜欢」"
                }
              >
                <Heart
                  size={16}
                  fill={detail.isFavorite ? "currentColor" : "none"}
                />
                <span>{detail.isFavorite ? "已喜欢" : "喜欢"}</span>
              </button>

              {canDownloadOnline && (
                <button
                  type="button"
                  className="btn btn-secondary"
                  disabled={preparingDownload}
                  onClick={() => void handleOpenDownloadPreview()}
                >
                  <Download size={16} />
                  <span>下载</span>
                </button>
              )}
            </div>
          </div>
        </div>
      </section>

      {editingOverrides && (
        <section className="modal-card">
          <div className="modal-header">
            <h3>编辑作品元数据</h3>
            <button
              type="button"
              className="btn btn-ghost btn-sm"
              onClick={() => setEditingOverrides(false)}
            >
              收起
            </button>
          </div>
          <div className="override-grid">
            <div className="override-row">
              <label>标题</label>
              <input
                type="text"
                className="input-text"
                value={ovTitle}
                onChange={(e) => setOvTitle(e.target.value)}
              />
              <button
                type="button"
                className="btn btn-primary btn-sm"
                onClick={() => void handleSaveOverrideField("title", "set", ovTitle)}
              >
                保存
              </button>
              <button
                type="button"
                className="btn btn-ghost btn-sm"
                onClick={() => void handleResetOverrideField("title")}
              >
                重置
              </button>
            </div>

            <div className="override-row">
              <label>社团名</label>
              <input
                type="text"
                className="input-text"
                value={ovCircle}
                onChange={(e) => setOvCircle(e.target.value)}
              />
              <button
                type="button"
                className="btn btn-primary btn-sm"
                onClick={() =>
                  void handleSaveOverrideField("circle_name", "set", ovCircle)
                }
              >
                保存
              </button>
              <button
                type="button"
                className="btn btn-secondary btn-sm"
                onClick={() =>
                  void handleSaveOverrideField("circle_name", "cleared", null)
                }
              >
                清空
              </button>
              <button
                type="button"
                className="btn btn-ghost btn-sm"
                onClick={() => void handleResetOverrideField("circle_name")}
              >
                重置
              </button>
            </div>

            <div className="override-row">
              <label>简介</label>
              <input
                type="text"
                className="input-text"
                value={ovDesc}
                onChange={(e) => setOvDesc(e.target.value)}
              />
              <button
                type="button"
                className="btn btn-primary btn-sm"
                onClick={() =>
                  void handleSaveOverrideField("description", "set", ovDesc)
                }
              >
                保存
              </button>
              <button
                type="button"
                className="btn btn-secondary btn-sm"
                onClick={() =>
                  void handleSaveOverrideField("description", "cleared", null)
                }
              >
                清空
              </button>
              <button
                type="button"
                className="btn btn-ghost btn-sm"
                onClick={() => void handleResetOverrideField("description")}
              >
                重置
              </button>
            </div>

            <div className="override-row">
              <label>声优 CV</label>
              <input
                type="text"
                className="input-text"
                value={ovVas}
                onChange={(e) => setOvVas(e.target.value)}
              />
              <button
                type="button"
                className="btn btn-primary btn-sm"
                onClick={() => {
                  const arr = ovVas
                    .split(",")
                    .map((s) => s.trim())
                    .filter(Boolean);
                  void handleSaveOverrideField(
                    "voice_actors",
                    "set",
                    JSON.stringify(arr)
                  );
                }}
              >
                保存
              </button>
              <button
                type="button"
                className="btn btn-ghost btn-sm"
                onClick={() => void handleResetOverrideField("voice_actors")}
              >
                重置
              </button>
            </div>

            <div className="override-row">
              <label>标签</label>
              <input
                type="text"
                className="input-text"
                value={ovTags}
                onChange={(e) => setOvTags(e.target.value)}
              />
              <button
                type="button"
                className="btn btn-primary btn-sm"
                onClick={() => {
                  const arr = ovTags
                    .split(",")
                    .map((s) => s.trim())
                    .filter(Boolean);
                  void handleSaveOverrideField(
                    "tags",
                    "set",
                    JSON.stringify(arr)
                  );
                }}
              >
                保存
              </button>
              <button
                type="button"
                className="btn btn-ghost btn-sm"
                onClick={() => void handleResetOverrideField("tags")}
              >
                重置
              </button>
            </div>
          </div>
        </section>
      )}

      {showAssocPanel && (
        <section className="modal-card">
          <div className="modal-header">
            <h3>作品与音轨版本关联</h3>
            <button
              type="button"
              className="btn btn-ghost btn-sm"
              onClick={() => setShowAssocPanel(false)}
            >
              收起
            </button>
          </div>

          <div className="override-grid">
            <div className="override-row">
              <label>关联在线作品 ID/RJ</label>
              <input
                type="text"
                className="input-text"
                placeholder={`输入在线作品号 (当前: ${detail.rjCode ?? "无"})`}
                value={assocOnlineInput}
                onChange={(e) => setAssocOnlineInput(e.target.value)}
              />
              <button
                type="button"
                className="btn btn-primary btn-sm"
                onClick={() => void handleAssociateOnlineByInput()}
              >
                确认关联
              </button>
            </div>

            <div className="override-row">
              <label>合并同版本音轨</label>
              <Select
                aria-label="保留的目标版本"
                value={mergeTargetVer}
                onValueChange={setMergeTargetVer}
                options={[{ value: "", label: "选择保留的目标版本..." }, ...allVersionsFlat.map(v => ({ value: v.versionId, label: v.label }))]}
              />
              <Select
                aria-label="合并入目标的源版本"
                value={mergeSourceVer}
                onValueChange={setMergeSourceVer}
                options={[{ value: "", label: "选择合并入目标的源版本..." }, ...allVersionsFlat.map(v => ({ value: v.versionId, label: v.label }))]}
              />
              <button
                type="button"
                className="btn btn-secondary btn-sm"
                onClick={() => void handleMergeVersions()}
              >
                确认合并
              </button>
            </div>
          </div>

          {assocHistory.length > 0 && (
            <div className="assoc-history-box">
              <h4>关联记录</h4>
              <ul className="bookmark-list">
                {assocHistory.map((h) => (
                  <li key={h.id} className="bookmark-item">
                    <span>
                      [{h.entityType === "work" ? "作品关联" : "版本合并"}]{" "}
                      {h.conflictNote ?? ""} ({h.createdAt})
                    </span>
                    {!h.undone ? (
                      <button
                        type="button"
                        className="btn btn-secondary btn-sm"
                        onClick={() => void handleUndoAssociation(h.id)}
                      >
                        <RotateCcw size={13} />
                        <span>撤销</span>
                      </button>
                    ) : (
                      <span className="text-muted">已撤销</span>
                    )}
                  </li>
                ))}
              </ul>
            </div>
          )}
        </section>
      )}

      {dlPreview && (
        <DownloadTaskModal
          preview={dlPreview}
          selectedIds={dlSelectedIds}
          onChangeSelectedIds={setDlSelectedIds}
          onConfirm={() => void handleConfirmDownload()}
          onClose={() => setDlPreview(null)}
        />
      )}

      {detail.directoryGroups.length > 0 && (
        <section className="directory-groups-section">
          <div className="group-tabs-top-row">
            <button
              type="button"
              className="group-collapse-toggle"
              onClick={() => setIsDirCollapsed((prev) => !prev)}
              aria-expanded={!isDirCollapsed}
              title={isDirCollapsed ? "点击展开目录列表" : "点击折叠目录列表"}
            >
              <FolderTree size={15} />
              <span>目录</span>
              {isDirCollapsed ? (
                <ChevronRight size={14} />
              ) : (
                <ChevronDown size={14} />
              )}
            </button>
            {!isDirCollapsed ? (
              <div className="group-tabs">
                {detail.directoryGroups.map((g) => (
                  <button
                    key={g.groupPath}
                    type="button"
                    className={`btn-chip ${
                      selectedGroupPath === g.groupPath ? "active" : ""
                    }`}
                    onClick={() => setSelectedGroupPath(g.groupPath)}
                  >
                    <span>
                      {g.label} ({g.trackCount} 轨 · {g.formats.join("/")})
                    </span>
                  </button>
                ))}
              </div>
            ) : (
              activeGroup && (
                <span className="group-collapsed-summary">
                  当前：{activeGroup.label} ({activeGroup.trackCount} 轨 ·{" "}
                  {activeGroup.formats.join("/")}) · 共{" "}
                  {detail.directoryGroups.length} 个目录
                </span>
              )
            )}
          </div>
          {selectedGroupPath !== undefined && (
            <div className="group-actions-second-row">
              <button
                type="button"
                className="btn btn-primary btn-sm"
                onClick={() => void handlePlayGroup(selectedGroupPath)}
              >
                <Play size={14} />
                <span>播放当前目录</span>
              </button>
            </div>
          )}
        </section>
      )}

      <section className="tracks-section">
        <table className="tracks-table">
          <thead>
            <tr>
              <th>音轨</th>
              <th>目录</th>
              <th>来源</th>
              <th>时长</th>
              <th>操作</th>
            </tr>
          </thead>
          <tbody>
            {filteredTracks.slice(0, visibleTrackLimit).map((track) =>
              track.versions.map((ver) => {
                return (
                  <tr key={ver.id} id={`bookmark-version-${ver.id}`} tabIndex={-1} className={`track-row ${focusVersionId === ver.id ? "bookmark-version-target" : ""}`}>
                    <td>
                      <div className="track-title-cell">
                        <span className="track-main-name">{track.title}</span>
                        {ver.associationState === "pending_confirm" && (
                          <span className="badge badge-warn">待确认变更</span>
                        )}

                      </div>
                    </td>
                    <td>{track.logicalDirPath || "根目录"}</td>
                    <td>
                      <div className="source-pills-row">
                        {ver.sources.map((src) => (
                          <button
                            key={src.id}
                            type="button"
                            className={`source-pill source-${src.sourceKind} ${
                              src.availability !== "available" ? "unavailable" : ""
                            }`}
                            disabled={src.availability !== "available"}
                            onClick={() => void handlePlayVersion(ver.id, src.id)}
                            title={
                              src.relativePath ??
                              src.remoteRelativePath ??
                              "点击使用该来源播放"
                            }
                          >
                            {src.sourceKind === "local_imported" ? (
                              <HardDrive size={12} />
                            ) : src.sourceKind === "local_downloaded" ? (
                              <CheckCircle2 size={12} />
                            ) : (
                              <Radio size={12} />
                            )}
                            <span>
                              {src.sourceKind === "local_imported"
                                ? "本地"
                                : src.sourceKind === "local_downloaded"
                                ? "已下载"
                                : "在线"}{" "}
                              ({src.format.toUpperCase()})
                              {src.availability !== "available" ? " [不可用]" : ""}
                            </span>
                          </button>
                        ))}
                      </div>
                    </td>
                    <td>
                      <span>
                        {ver.positionMs > 0
                          ? `${formatDurationMs(ver.positionMs)} / `
                          : ""}
                        {formatDurationMs(ver.durationMs)}
                      </span>
                      {ver.completed && (
                        <span className="badge badge-completed">已完成</span>
                      )}
                    </td>
                    <td>
                      <div className="track-actions">
                        <button
                          type="button"
                          className="btn btn-primary btn-sm"
                          onClick={() =>
                            void handlePlayGroup(track.logicalDirPath, ver.id)
                          }
                          title="从该音轨开始播放所属目录"
                        >
                          <Play size={13} />
                          <span>播放</span>
                        </button>
                        <button
                          type="button"
                          className="btn btn-ghost btn-sm"
                          onClick={() => void handleAppendQueue(ver.id, null)}
                          title="加入播放队列"
                        >
                          <Plus size={13} />
                          <span>入队</span>
                        </button>
                        <button
                          type="button"
                          className="btn btn-ghost btn-sm"
                          onClick={() => void handleFavoriteTrack(ver.id)}
                          disabled={favoritePending !== null}
                          aria-pressed={!!ver.isFavorite}
                          title={ver.isFavorite ? "取消喜爱单曲" : "喜爱单曲，也将作品加入喜爱"}
                          aria-label={`${ver.isFavorite ? "取消喜爱" : "喜爱"}单曲：${track.title}`}
                        >
                          <Heart size={14} fill={ver.isFavorite ? "currentColor" : "none"} />
                        </button>
                      </div>
                    </td>
                  </tr>
                );
              })
            )}
          </tbody>
        </table>
        {filteredTracks.length > visibleTrackLimit && (
          <div className="pagination-row">
            <button
              type="button"
              className="btn btn-secondary btn-sm"
              onClick={() => setVisibleTrackLimit((n) => n + 100)}
            >
              展开更多音轨 ({visibleTrackLimit} / {filteredTracks.length})
            </button>
          </div>
        )}
      </section>
    </div>
  );
};
