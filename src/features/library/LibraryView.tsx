import React, { useState, useEffect, useCallback, useRef } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import {
  FolderPlus,
  Library,
  Search,
  RefreshCw,
  Heart,
  Bookmark,
  Play,
  HardDrive,
  Radio,
  CheckCircle2,
  AlertCircle,
  Globe,
  Download,
  Trash2,
  FolderTree,
  MoreVertical,
} from "lucide-react";
import {
  WorkSummary,
  ImportPreviewDto,
  ScanReportDto,
  PlaybackSnapshot,
  WorkDetailDto,
  DownloadPreviewDto,
  DownloadTaskDto,
  callCommand,
  formatDurationMs,
  normalizeAppError,
  AppError,
} from "../../lib/api";
import { DownloadTaskModal } from "../../components/DownloadTaskModal";

interface LibraryViewProps {
  filterMode: "all" | "local" | "favorites" | "recent";
  refreshTick?: number;
  externalSearch?: { keyword: string; seq: number } | null;
  canReturnToDetail?: boolean;
  onReturnToDetail?: () => void;
  onSelectWork: (workId: string) => void;
  onSnapshotChange: (snap: PlaybackSnapshot) => void;
  onOpenDownloads?: () => void;
  onError: (err: AppError) => void;
}

const PAGE_SIZE = 24;

interface ContextMenuState {
  work: WorkSummary;
  x: number;
  y: number;
}

export const LibraryView: React.FC<LibraryViewProps> = ({
  filterMode,
  refreshTick,
  externalSearch,
  canReturnToDetail,
  onReturnToDetail,
  onSelectWork,
  onSnapshotChange,
  onOpenDownloads,
  onError,
}) => {
  const [works, setWorks] = useState<WorkSummary[]>([]);
  const [loading, setLoading] = useState(false);
  const [searchQuery, setSearchQuery] = useState("");
  const [visibleCount, setVisibleCount] = useState(PAGE_SIZE);
  const [playingAllFavorites, setPlayingAllFavorites] = useState(false);

  // Import modal state (Smart Work Root Resolution)
  const [importMode, setImportMode] = useState<
    "single_work" | "library_root" | null
  >(null);
  const [importPath, setImportPath] = useState("");
  const [smartLevelOffset, setSmartLevelOffset] = useState<number>(0);
  const [preview, setPreview] = useState<ImportPreviewDto | null>(null);
  const [customTitles, setCustomTitles] = useState<Record<string, string>>({});
  const [selectedRels, setSelectedRels] = useState<Record<string, boolean>>({});
  const [previewing, setPreviewing] = useState(false);
  const [scanning, setScanning] = useState(false);

  // Quick open online work by ID/RJ
  const [quickOnlineId, setQuickOnlineId] = useState("");
  const [openingOnline, setOpeningOnline] = useState(false);

  // Right-click context menu state
  const [contextMenu, setContextMenu] = useState<ContextMenuState | null>(null);

  // Download preview modal from context menu
  const [dlPreview, setDlPreview] = useState<DownloadPreviewDto | null>(null);
  const [dlSelectedIds, setDlSelectedIds] = useState<Record<string, boolean>>(
    {}
  );
  const [, setBusyWorkId] = useState<string | null>(null);

  // Remove/Delete confirmation modal state
  const [deleteTargetWork, setDeleteTargetWork] = useState<WorkSummary | null>(
    null
  );
  const [alsoDeleteDownloaded, setAlsoDeleteDownloaded] = useState(false);

  // Sync external CV / Tag search keyword when triggered from WorkDetailView
  useEffect(() => {
    if (externalSearch && externalSearch.seq > 0) {
      setSearchQuery(externalSearch.keyword);
    }
  }, [externalSearch]);

  const requestSeq = useRef(0);
  const loadWorks = useCallback(async () => {
    const seq = ++requestSeq.current;
    setLoading(true);
    try {
      const list = await callCommand<WorkSummary[]>("list_works", {
        filter: filterMode,
        search: searchQuery.trim() || null,
      });
      if (seq === requestSeq.current) setWorks(list);
    } catch (e) {
      if (seq === requestSeq.current) onError(normalizeAppError(e));
    } finally {
      if (seq === requestSeq.current) setLoading(false);
    }
  }, [filterMode, searchQuery, onError]);

  useEffect(() => {
    setVisibleCount(PAGE_SIZE);
  }, [filterMode, searchQuery]);

  useEffect(() => {
    void loadWorks();
    return () => { requestSeq.current += 1; };
  }, [loadWorks, refreshTick]);

  useEffect(() => {
    if (!contextMenu) return;
    const handleKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        setContextMenu(null);
      }
    };
    window.addEventListener("keydown", handleKey);
    return () => window.removeEventListener("keydown", handleKey);
  }, [contextMenu]);

  const handleChooseFolder = async (
    mode: "single_work" | "library_root",
    chooseSingleFile = false
  ) => {
    setImportMode(mode);
    setSmartLevelOffset(0);
    setPreview(null);
    try {
      const picked = await open({
        directory: !chooseSingleFile,
        multiple: false,
        title: chooseSingleFile
          ? "选择单个音频或字幕文件"
          : mode === "single_work"
          ? "选择作品文件夹导入"
          : "选择作品库根目录导入",
      });
      if (typeof picked === "string" && picked.trim()) {
        setImportPath(picked);
        await handlePreviewPath(picked, mode, 0);
      }
    } catch {
      // Allow manual path entry in modal if native dialog is canceled or unavailable
    }
  };

  const handlePreviewPath = async (
    pathStr: string,
    mode: "single_work" | "library_root",
    levelOffset = smartLevelOffset
  ) => {
    if (!pathStr.trim()) return;
    setPreviewing(true);
    try {
      const prev =
        mode === "single_work"
          ? await callCommand<ImportPreviewDto>("preview_smart_import", {
              selectedPath: pathStr.trim(),
              levelOffset,
            })
          : await callCommand<ImportPreviewDto>("preview_import", {
              rootPath: pathStr.trim(),
              rootMode: mode,
            });
      setPreview(prev);
      const titles: Record<string, string> = {};
      const sel: Record<string, boolean> = {};
      for (const c of prev.candidates) {
        titles[c.relativeWorkPath] = c.proposedTitle;
        sel[c.relativeWorkPath] = true;
      }
      setCustomTitles(titles);
      setSelectedRels(sel);
    } catch (e) {
      onError(normalizeAppError(e));
    } finally {
      setPreviewing(false);
    }
  };

  const handleShiftSmartLevel = async (delta: number) => {
    if (!importPath.trim()) return;
    const nextOffset = smartLevelOffset + delta;
    setSmartLevelOffset(nextOffset);
    await handlePreviewPath(importPath, "single_work", nextOffset);
  };

  const handleExecuteImport = async () => {
    if (!preview || !importMode) return;
    const chosen: Array<[string, string]> = preview.candidates
      .filter((c) => selectedRels[c.relativeWorkPath])
      .map((c) => [
        c.relativeWorkPath,
        (customTitles[c.relativeWorkPath] ?? c.proposedTitle).trim() ||
          c.proposedTitle,
      ]);

    if (chosen.length === 0) return;
    setScanning(true);
    try {
      await callCommand<ScanReportDto>("scan_and_import", {
        rootPath: preview.rootPath,
        rootMode: importMode,
        selectedCandidates: chosen,
      });
      setPreview(null);
      setImportMode(null);
      await loadWorks();
    } catch (e) {
      onError(normalizeAppError(e));
    } finally {
      setScanning(false);
    }
  };

  const handleCancelScan = async () => {
    try {
      await callCommand("cancel_scan");
    } catch (e) {
      onError(normalizeAppError(e));
    }
  };

  const handleQuickOpenOnline = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!quickOnlineId.trim()) return;
    setOpeningOnline(true);
    try {
      const detail = await callCommand<WorkDetailDto>("open_online_work", {
        workIdOrRj: quickOnlineId.trim(),
      });
      setQuickOnlineId("");
      onSelectWork(detail.id);
    } catch (err) {
      onError(normalizeAppError(err));
    } finally {
      setOpeningOnline(false);
    }
  };

  const handleResumeWork = async (work: WorkSummary, e?: React.MouseEvent) => {
    e?.stopPropagation();
    setContextMenu(null);
    if (!work.lastVersionId) {
      onSelectWork(work.id);
      return;
    }
    try {
      const snap = await callCommand<PlaybackSnapshot>("play_version", {
        versionId: work.lastVersionId,
        preferredSourceId: null,
      });
      onSnapshotChange(snap);
    } catch (err) {
      onError(normalizeAppError(err));
    }
  };

  const handleToggleFavorite = async (
    workId: string,
    e?: React.MouseEvent
  ) => {
    e?.stopPropagation();
    setContextMenu(null);
    try {
      await callCommand<boolean>("toggle_favorite", { workId });
      await loadWorks();
    } catch (err) {
      onError(normalizeAppError(err));
    }
  };

  const handleToggleCollected = async (
    workId: string,
    e?: React.MouseEvent
  ) => {
    e?.stopPropagation();
    setContextMenu(null);
    try {
      await callCommand<boolean>("toggle_collected", { workId });
      await loadWorks();
    } catch (err) {
      onError(normalizeAppError(err));
    }
  };

  const openContextMenuAt = (
    work: WorkSummary,
    clientX: number,
    clientY: number
  ) => {
    const clampedX = Math.min(
      clientX,
      Math.max(16, window.innerWidth - 260)
    );
    const clampedY = Math.min(
      clientY,
      Math.max(16, window.innerHeight - 310)
    );
    setContextMenu({ work, x: clampedX, y: clampedY });
  };

  // Context Menu Action: Open unified download tree modal ("创建下载任务")
  const handleOpenDownloadModal = async (work: WorkSummary) => {
    setContextMenu(null);
    setBusyWorkId(work.id);
    try {
      const prev = await callCommand<DownloadPreviewDto>(
        "prepare_work_download",
        {
          workId: work.id,
          rjOrId: work.rjCode ?? work.sourceWorkId ?? null,
        }
      );
      setDlPreview(prev);
      const sel: Record<string, boolean> = {};
      for (const item of prev.items) {
        sel[item.sourceId] = !item.alreadyDownloaded && !item.alreadyQueued;
      }
      setDlSelectedIds(sel);
      await loadWorks();
    } catch (err) {
      onError(normalizeAppError(err));
    } finally {
      setBusyWorkId(null);
    }
  };

  const handleConfirmDownloadModal = async () => {
    if (!dlPreview) return;
    const chosen = dlPreview.items
      .filter((i) => dlSelectedIds[i.sourceId])
      .map((i) => i.sourceId);
    if (chosen.length === 0) return;
    try {
      await callCommand<DownloadTaskDto[]>("enqueue_downloads", {
        workId: dlPreview.workId,
        sourceIds: chosen,
      });
      setDlPreview(null);
      await loadWorks();
      onOpenDownloads?.();
    } catch (err) {
      onError(normalizeAppError(err));
    }
  };

  // Context Menu Action: Sync metadata & cover from asmr.one
  const handleSyncWorkMeta = async (work: WorkSummary) => {
    setContextMenu(null);
    setBusyWorkId(work.id);
    try {
      await callCommand<WorkDetailDto>("sync_work_from_asmr_one", {
        workId: work.id,
        rjOrId: work.rjCode ?? work.sourceWorkId ?? null,
        preferOnlineCover: true,
      });
      await loadWorks();
    } catch (err) {
      onError(normalizeAppError(err));
    } finally {
      setBusyWorkId(null);
    }
  };

  // Context Menu Action: Delete only downloaded offline files
  const handleDeleteDownloadedOnly = async (work: WorkSummary) => {
    setContextMenu(null);
    setBusyWorkId(work.id);
    try {
      await callCommand<WorkDetailDto>("delete_work_downloaded_files", {
        workId: work.id,
      });
      await loadWorks();
    } catch (err) {
      onError(normalizeAppError(err));
    } finally {
      setBusyWorkId(null);
    }
  };

  // Context Menu Action: Confirm remove work from library
  const handleConfirmDeleteWork = async () => {
    if (!deleteTargetWork) return;
    const target = deleteTargetWork;
    setDeleteTargetWork(null);
    setBusyWorkId(target.id);
    try {
      const snap = await callCommand<PlaybackSnapshot>(
        "delete_work_from_library",
        {
          workId: target.id,
          deleteDownloadedFiles: alsoDeleteDownloaded,
        }
      );
      onSnapshotChange(snap);
      await loadWorks();
    } catch (err) {
      onError(normalizeAppError(err));
    } finally {
      setBusyWorkId(null);
    }
  };

  // Play all favorite works in order, preferring With-SE + WAV format
  const handlePlayAllFavorites = async () => {
    if (works.length === 0 || playingAllFavorites) return;
    setPlayingAllFavorites(true);
    try {
      const workIds = works.map((w) => w.id);
      const snap = await callCommand<PlaybackSnapshot | null>(
        "play_favorites_queue",
        { workIds }
      );
      if (snap) {
        onSnapshotChange(snap);
      }
    } catch (err) {
      onError(normalizeAppError(err));
    } finally {
      setPlayingAllFavorites(false);
    }
  };

  const viewTitle =
    filterMode === "favorites"
      ? "喜欢"
      : filterMode === "recent"
      ? "最近收听"
      : "我的音频库";

  const visibleWorks = works.slice(0, visibleCount);
  const selectedImportCount = preview
    ? preview.candidates.filter((c) => selectedRels[c.relativeWorkPath]).length
    : 0;

  return (
    <div className="page-container">
      {/* M3 Top App Bar */}
      <header className="top-app-bar" aria-label="音频库顶栏">
        <div className="top-bar-primary-row">
          <div className="top-bar-title-group">
            <h2 className="top-bar-title">{viewTitle}</h2>
            <span className="top-bar-count">共 {works.length} 部作品</span>
          </div>

          <div className="toolbar-actions">
            {canReturnToDetail && onReturnToDetail && (
              <button
                type="button"
                className="btn btn-secondary btn-sm"
                onClick={onReturnToDetail}
              >
                <span>返回原作品详情</span>
              </button>
            )}
            {filterMode === "favorites" && (
              <button
                type="button"
                className="btn btn-primary btn-sm"
                disabled={works.length === 0 || playingAllFavorites}
                onClick={() => void handlePlayAllFavorites()}
                aria-label="一键播放全部喜欢作品"
                title="按列表顺序将全部喜欢作品排入播放队列并立即播放（默认优选有音效 + WAV 格式）"
              >
                <Play size={15} />
                <span>
                  {playingAllFavorites ? "正在编排队列…" : "一键播放全部"}
                </span>
              </button>
            )}
            {filterMode === "all" && (
              <>
                <button
                  type="button"
                  className="btn btn-primary btn-sm"
                  onClick={() => void handleChooseFolder("single_work", false)}
                >
                  <FolderPlus size={15} />
                  <span>导入作品文件夹</span>
                </button>
                <button
                  type="button"
                  className="btn btn-secondary btn-sm"
                  onClick={() => void handleChooseFolder("single_work", true)}
                >
                  <FolderTree size={15} />
                  <span>选单文件智能整包导入</span>
                </button>
                <button
                  type="button"
                  className="btn btn-secondary btn-sm"
                  onClick={() => void handleChooseFolder("library_root", false)}
                >
                  <Library size={15} />
                  <span>导入作品库根目录</span>
                </button>
              </>
            )}
            <button
              type="button"
              className="btn-icon"
              onClick={() => void loadWorks()}
              aria-label="刷新作品库"
              title="刷新作品库"
            >
              <RefreshCw size={15} />
            </button>
          </div>
        </div>

        <div className="top-bar-controls-row">
          <div className="search-input-wrap">
            <Search size={15} className="text-muted" />
            <input
              type="text"
              className="input-text"
              placeholder="搜索标题、RJ号、社团、声优或标签..."
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
            />
          </div>

          {filterMode === "all" && (
            <form
              onSubmit={(e) => void handleQuickOpenOnline(e)}
              className="quick-online-form"
            >
              <Globe size={15} className="text-muted" />
              <input
                type="text"
                className="input-text input-sm"
                placeholder="输入 RJ 号或 ID 打开在线作品"
                value={quickOnlineId}
                onChange={(e) => setQuickOnlineId(e.target.value)}
              />
              <button
                type="submit"
                className="btn btn-secondary btn-sm"
                disabled={openingOnline || !quickOnlineId.trim()}
              >
                打开作品
              </button>
            </form>
          )}
        </div>
      </header>

      {/* 本地目录导入预览弹窗 (支持单文件/深层子目录智能回溯作品根目录) */}
      {importMode && (
        <div className="modal-card">
          <div className="modal-header">
            <h3>
              {importMode === "single_work"
                ? "导入作品文件夹（支持单文件/深层子目录自动回溯整部作品）"
                : "导入作品库根目录（按直接子目录分组预览）"}
            </h3>
            <button
              type="button"
              className="btn btn-ghost btn-sm"
              onClick={() => {
                setImportMode(null);
                setPreview(null);
              }}
            >
              取消
            </button>
          </div>
          <div className="modal-path-row">
            <input
              type="text"
              className="input-text"
              placeholder="输入或粘贴本地绝对路径（文件夹、子目录或单个音频文件），例如 D:\ASMR\RJ01705068\WAV\01.wav"
              value={importPath}
              onChange={(e) => setImportPath(e.target.value)}
            />
            <button
              type="button"
              className="btn btn-secondary btn-sm"
              disabled={previewing || !importPath.trim()}
              onClick={() => void handlePreviewPath(importPath, importMode, 0)}
            >
              {previewing ? "正在解析目录…" : "预览并导入"}
            </button>
          </div>

          {preview && (
            <div className="preview-section">
              {importMode === "single_work" && (
                <div className="smart-root-banner">
                  <div className="smart-root-info">
                    <span className="badge badge-local">智能目录树定界</span>
                    <span className="smart-root-path" title={preview.rootPath}>
                      作品根目录：<strong>{preview.rootPath}</strong>
                    </span>
                    {preview.detectionReason && (
                      <span className="text-muted">
                        ({preview.detectionReason})
                      </span>
                    )}
                  </div>
                  <div className="track-actions">
                    <button
                      type="button"
                      className="btn btn-secondary btn-sm"
                      disabled={previewing}
                      onClick={() => void handleShiftSmartLevel(1)}
                      title="将作品根目录边界向父目录上移一层"
                    >
                      <span>向上一层 (父目录)</span>
                    </button>
                    <button
                      type="button"
                      className="btn btn-ghost btn-sm"
                      disabled={previewing}
                      onClick={() => void handleShiftSmartLevel(-1)}
                      title="将作品根目录边界向原始选中路径下移一层"
                    >
                      <span>向下一层 (子目录)</span>
                    </button>
                  </div>
                </div>
              )}

              {preview.overlappingRoots.length > 0 && (
                <div className="warning-banner">
                  <AlertCircle size={15} />
                  <span>
                    注意：该路径与已导入根目录存在重叠（
                    {preview.overlappingRoots.join(", ")}
                    ），入库时将按唯一路径约束自动去重。
                  </span>
                </div>
              )}

              {preview.candidates.length === 0 ? (
                <div className="empty-box">
                  该目录下未检测到支持的音频（MP3/FLAC/WAV/M4A/OGG/OPUS）或字幕文件
                </div>
              ) : (
                <table className="preview-table">
                  <thead>
                    <tr>
                      <th>选择</th>
                      <th>相对目录</th>
                      <th>作品标题（可修改）</th>
                      <th>识别 RJ</th>
                      <th>音轨 / 字幕 / 封面</th>
                      <th>状态</th>
                    </tr>
                  </thead>
                  <tbody>
                    {preview.candidates.map((cand) => {
                      const isSel = Boolean(
                        selectedRels[cand.relativeWorkPath]
                      );
                      return (
                        <tr key={cand.relativeWorkPath}>
                          <td>
                            <button
                              type="button"
                              className={`dsp-scope-toggle-btn ${
                                isSel ? "is-lit" : "is-dim"
                              }`}
                              aria-pressed={isSel}
                              onClick={() =>
                                setSelectedRels((prev) => ({
                                  ...prev,
                                  [cand.relativeWorkPath]: !isSel,
                                }))
                              }
                            >
                              <span>{isSel ? "已选中" : "未选中"}</span>
                            </button>
                          </td>
                          <td>{cand.relativeWorkPath}</td>
                          <td>
                            <input
                              type="text"
                              className="input-text input-sm"
                              value={
                                customTitles[cand.relativeWorkPath] ??
                                cand.proposedTitle
                              }
                              onChange={(e) =>
                                setCustomTitles((prev) => ({
                                  ...prev,
                                  [cand.relativeWorkPath]: e.target.value,
                                }))
                              }
                            />
                          </td>
                          <td>{cand.detectedRjCode ?? "—"}</td>
                          <td className="tabular-nums">
                            {cand.audioCount} 音轨 / {cand.subtitleCount} 字幕 /{" "}
                            {cand.coverDetected ? "含封面" : "无封面"}
                          </td>
                          <td>{cand.alreadyIndexed ? "增量更新" : "新入库"}</td>
                        </tr>
                      );
                    })}
                  </tbody>
                </table>
              )}

              <div className="modal-footer">
                {scanning ? (
                  <button
                    type="button"
                    className="btn btn-secondary btn-sm"
                    onClick={() => void handleCancelScan()}
                  >
                    取消扫描
                  </button>
                ) : (
                  <button
                    type="button"
                    className="btn btn-primary"
                    disabled={selectedImportCount === 0}
                    onClick={() => void handleExecuteImport()}
                  >
                    导入选中作品 ({selectedImportCount})
                  </button>
                )}
              </div>
            </div>
          )}
        </div>
      )}

      {/* 右键菜单触发的统一“创建下载任务”树形二级弹窗 */}
      {dlPreview && (
        <DownloadTaskModal
          preview={dlPreview}
          selectedIds={dlSelectedIds}
          onChangeSelectedIds={setDlSelectedIds}
          onConfirm={() => void handleConfirmDownloadModal()}
          onClose={() => setDlPreview(null)}
        />
      )}

      {/* 从音频库移除作品确认弹窗（M3 安全边界法则） */}
      {deleteTargetWork && (
        <section className="modal-card">
          <div className="modal-header">
            <h3>从音频库移除作品 · 《{deleteTargetWork.title}》</h3>
            <button
              type="button"
              className="btn btn-ghost btn-sm"
              onClick={() => setDeleteTargetWork(null)}
            >
              取消
            </button>
          </div>
          <p className="text-muted">
            此操作将从软件数据库中移除该作品的索引、播放队列项与关联记录，
            <strong>绝不会删除磁盘上的本地导入原始音频文件</strong>。
          </p>
          {deleteTargetWork.hasDownloaded && (
            <div className="track-actions">
              <button
                type="button"
                className={`dsp-scope-toggle-btn ${
                  alsoDeleteDownloaded ? "is-lit" : "is-dim"
                }`}
                aria-pressed={alsoDeleteDownloaded}
                onClick={() => setAlsoDeleteDownloaded((v) => !v)}
              >
                <span>
                  同时删除离线文件（仅清理下载目录内的软件下载副本与 .part
                  临时文件）
                </span>
              </button>
            </div>
          )}
          <div className="modal-footer">
            <button
              type="button"
              className="btn btn-ghost"
              onClick={() => setDeleteTargetWork(null)}
            >
              取消
            </button>
            <button
              type="button"
              className="btn btn-danger"
              onClick={() => void handleConfirmDeleteWork()}
            >
              <Trash2 size={14} />
              <span>
                {alsoDeleteDownloaded
                  ? "移除作品并删除离线文件"
                  : "确认移除作品索引"}
              </span>
            </button>
          </div>
        </section>
      )}

      {works.length === 0 && !loading ? (
        <div className="empty-state-box">
          <h3>
            {filterMode === "favorites"
              ? "暂无喜欢的作品"
              : filterMode === "recent"
              ? "暂无收听记录"
              : "暂无收藏或导入的作品"}
          </h3>
        </div>
      ) : (
        <>
          <div className="work-grid">
            {visibleWorks.map((work) => (
              <article
                key={work.id}
                className="work-card"
                onClick={() => onSelectWork(work.id)}
                onContextMenu={(e) => {
                  e.preventDefault();
                  e.stopPropagation();
                  openContextMenuAt(work, e.clientX, e.clientY);
                }}
              >
                {work.coverRef && (
                  <div className="work-card-cover-wrap">
                    <img
                      src={work.coverRef}
                      alt={work.title}
                      className="work-card-cover"
                      loading="lazy"
                      onError={(e) => {
                        (
                          e.currentTarget.parentElement as HTMLElement
                        ).style.display = "none";
                      }}
                    />
                  </div>
                )}

                <div className="work-card-header">
                  <div className="work-badges">
                    {work.rjCode && (
                      <span className="badge badge-rj">{work.rjCode}</span>
                    )}
                    {work.hasLocal && (
                      <span className="badge badge-local">
                        <HardDrive size={11} />
                        <span>本地</span>
                      </span>
                    )}
                    {work.hasDownloaded && (
                      <span className="badge badge-downloaded">
                        <CheckCircle2 size={11} />
                        <span>已下载</span>
                      </span>
                    )}
                    {work.hasOnline && (
                      <span className="badge badge-online">
                        <Radio size={11} />
                        <span>在线</span>
                      </span>
                    )}
                    {work.availability !== "available" && (
                      <span className="badge badge-warn">来源缺失</span>
                    )}
                  </div>

                  <div className="work-card-header-actions">
                    <button
                      type="button"
                      className={`btn-fav ${work.isCollected ? "active" : ""}`}
                      onClick={(e) => void handleToggleCollected(work.id, e)}
                      aria-label={work.isCollected ? "取消收藏" : "收藏到音频库"}
                      title={work.isCollected ? "取消收藏" : "收藏到音频库"}
                    >
                      <Bookmark size={15} />
                    </button>
                    <button
                      type="button"
                      className={`btn-fav ${work.isFavorite ? "active" : ""}`}
                      onClick={(e) => void handleToggleFavorite(work.id, e)}
                      aria-label={work.isFavorite ? "取消喜欢" : "加入喜欢"}
                      title={work.isFavorite ? "取消喜欢" : "加入喜欢"}
                    >
                      <Heart size={15} />
                    </button>
                    <button
                      type="button"
                      className="btn-icon-sm"
                      onClick={(e) => {
                        e.stopPropagation();
                        openContextMenuAt(work, e.clientX, e.clientY);
                      }}
                      aria-label="打开作品操作菜单"
                      title="更多操作"
                    >
                      <MoreVertical size={15} />
                    </button>
                  </div>
                </div>

                <h3 className="work-card-title">{work.title}</h3>

                <div className="work-card-meta">
                  {work.circleName && <span>社团: {work.circleName}</span>}
                  {work.voiceActors.length > 0 && (
                    <span className="cv-list">
                      CV:{" "}
                      {work.voiceActors.map((va) => (
                        <span key={va} className="cv-pill">
                          {va}
                        </span>
                      ))}
                    </span>
                  )}
                </div>

                {work.tags.length > 0 && (
                  <div className="work-tags">
                    {work.tags.slice(0, 6).map((tag) => (
                      <span key={tag} className="tag-pill">
                        {tag}
                      </span>
                    ))}
                  </div>
                )}

                {work.lastTrackTitle && (
                  <div className="work-resume-row">
                    <button
                      type="button"
                      className="btn-resume-pill"
                      onClick={(e) => void handleResumeWork(work, e)}
                    >
                      <Play size={13} />
                      <span>
                        继续收听 · {work.lastTrackTitle} ·{" "}
                        {formatDurationMs(work.lastPositionMs)}
                      </span>
                    </button>
                  </div>
                )}
              </article>
            ))}
          </div>

          {works.length > visibleCount && (
            <div className="pagination-row">
              <button
                type="button"
                className="btn btn-secondary"
                onClick={() => setVisibleCount((c) => c + PAGE_SIZE)}
              >
                加载更多 ({visibleCount} / {works.length})
              </button>
            </div>
          )}
        </>
      )}

      {/* 右键浮动菜单 (M3 Surface Container High) */}
      {contextMenu && (
        <>
          <div
            className="context-menu-backdrop"
            onClick={() => setContextMenu(null)}
            onContextMenu={(e) => {
              e.preventDefault();
              setContextMenu(null);
            }}
          />
          <div
            className="m3-context-menu"
            role="menu"
            aria-label="作品操作菜单"
            style={{ left: `${contextMenu.x}px`, top: `${contextMenu.y}px` }}
          >
            <div className="m3-context-menu-header">
              <div className="m3-context-menu-title">
                {contextMenu.work.title}
              </div>
              <div className="m3-context-menu-sub">
                {contextMenu.work.rjCode ?? "作品"}
              </div>
            </div>

            <button
              type="button"
              className="m3-menu-item"
              role="menuitem"
              onClick={() => {
                const wid = contextMenu.work.id;
                setContextMenu(null);
                onSelectWork(wid);
              }}
            >
              <FolderTree size={15} />
              <span>打开作品详情</span>
            </button>

            {contextMenu.work.lastVersionId && (
              <button
                type="button"
                className="m3-menu-item"
                role="menuitem"
                onClick={() => void handleResumeWork(contextMenu.work)}
              >
                <Play size={15} />
                <span>继续收听</span>
              </button>
            )}

            <button
              type="button"
              className="m3-menu-item"
              role="menuitem"
              onClick={() => void handleToggleCollected(contextMenu.work.id)}
            >
              <Bookmark size={15} />
              <span>
                {contextMenu.work.isCollected ? "取消收藏" : "收藏到音频库"}
              </span>
            </button>

            <button
              type="button"
              className="m3-menu-item"
              role="menuitem"
              onClick={() => void handleToggleFavorite(contextMenu.work.id)}
            >
              <Heart size={15} />
              <span>
                {contextMenu.work.isFavorite ? "取消喜欢" : "加入喜欢"}
              </span>
            </button>

            {(!contextMenu.work.hasLocal ||
              contextMenu.work.rjCode ||
              contextMenu.work.hasOnline) && <div className="m3-menu-divider" />}

            {!contextMenu.work.hasLocal &&
              (contextMenu.work.hasOnline || Boolean(contextMenu.work.rjCode)) && (
                <button
                  type="button"
                  className="m3-menu-item"
                  role="menuitem"
                  onClick={() => void handleOpenDownloadModal(contextMenu.work)}
                >
                  <Download size={15} />
                  <span>创建下载任务…</span>
                </button>
              )}

            {(contextMenu.work.rjCode || contextMenu.work.hasOnline) && (
              <button
                type="button"
                className="m3-menu-item"
                role="menuitem"
                onClick={() => void handleSyncWorkMeta(contextMenu.work)}
              >
                <RefreshCw size={15} />
                <span>同步 asmr.one 封面与标签</span>
              </button>
            )}

            <div className="m3-menu-divider" />

            {contextMenu.work.hasDownloaded && (
              <button
                type="button"
                className="m3-menu-item danger"
                role="menuitem"
                onClick={() =>
                  void handleDeleteDownloadedOnly(contextMenu.work)
                }
              >
                <Trash2 size={15} />
                <span>删除离线文件</span>
              </button>
            )}

            <button
              type="button"
              className="m3-menu-item danger"
              role="menuitem"
              onClick={() => {
                const target = contextMenu.work;
                setContextMenu(null);
                setAlsoDeleteDownloaded(false);
                setDeleteTargetWork(target);
              }}
            >
              <Trash2 size={15} />
              <span>从音频库移除…</span>
            </button>
          </div>
        </>
      )}
    </div>
  );
};
