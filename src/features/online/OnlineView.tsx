import React, { useEffect, useRef, useState, useCallback } from "react";
import {
  Search,
  RefreshCw,
  ExternalLink,
  Subtitles,
  Flame,
  Trophy,
  Star,
  Sparkles,
  Calendar,
  MessageSquare,
  Download,
  FolderTree,
  Eye,
  EyeOff,
} from "lucide-react";
import {
  ProviderWork,
  Page,
  WorkDetailDto,
  DownloadPreviewDto,
  DownloadTaskDto,
  AppSettingsDto,
  callCommand,
  normalizeAppError,
  AppError,
} from "../../lib/api";
import { DownloadTaskModal } from "../../components/DownloadTaskModal";

interface OnlineViewProps {
  externalSearch?: { keyword: string; seq: number } | null;
  canReturnToDetail?: boolean;
  onReturnToDetail?: () => void;
  onOpenWorkDetail: (workId: string) => void;
  onOpenDownloads?: () => void;
  onError: (err: AppError) => void;
}

interface OnlineContextMenuState {
  work: ProviderWork;
  x: number;
  y: number;
}

export const OnlineView: React.FC<OnlineViewProps> = ({
  externalSearch,
  canReturnToDetail,
  onReturnToDetail,
  onOpenWorkDetail,
  onOpenDownloads,
  onError,
}) => {
  const [keyword, setKeyword] = useState("");
  const [committedKeyword, setCommittedKeyword] = useState("");
  // Default to "popular" so asmr.one popular works load immediately
  const [order, setOrder] = useState("popular");
  const [sort, setSort] = useState("desc");
  const [subtitleOnly, setSubtitleOnly] = useState(false);
  const [nsfwEnabled, setNsfwEnabled] = useState(false);
  const [page, setPage] = useState(1);

  const [works, setWorks] = useState<ProviderWork[]>([]);
  const [totalCount, setTotalCount] = useState(0);
  const [loading, setLoading] = useState(false);

  const [openingId, setOpeningId] = useState<string | null>(null);
  const [downloadingId, setDownloadingId] = useState<string | null>(null);
  const [dlPreview, setDlPreview] = useState<DownloadPreviewDto | null>(null);
  const [dlSelectedIds, setDlSelectedIds] = useState<Record<string, boolean>>(
    {}
  );
  const [pageSize, setPageSize] = useState(12);
  const [contextMenu, setContextMenu] = useState<OnlineContextMenuState | null>(
    null
  );

  useEffect(() => {
    void callCommand<AppSettingsDto | null>("get_app_settings")
      .then((s) => {
        if (s && typeof s.nsfwEnabled === "boolean") {
          setNsfwEnabled(s.nsfwEnabled);
        }
      })
      .catch(() => {});
  }, []);

  // Sync external CV / Tag search keyword when triggered from WorkDetailView
  useEffect(() => {
    if (externalSearch && externalSearch.seq > 0) {
      setKeyword(externalSearch.keyword);
      setCommittedKeyword(externalSearch.keyword);
      setPage(1);
    }
  }, [externalSearch]);

  // Request sequence guard so late responses from older queries/pages never overwrite newer pages
  const reqSeqRef = useRef(0);

  const fetchPage = useCallback(
    async (targetPage: number, append: boolean) => {
      const mySeq = ++reqSeqRef.current;
      setLoading(true);
      try {
        const res = committedKeyword.trim()
          ? await callCommand<Page<ProviderWork>>("search_online_works", {
              keyword: committedKeyword.trim(),
              page: targetPage,
              order,
              sort,
              subtitleOnly,
            })
          : await callCommand<Page<ProviderWork>>("browse_online_works", {
              page: targetPage,
              order,
              sort,
              subtitleOnly,
            });

        // Ignore stale response if a newer query/page was triggered
        if (reqSeqRef.current !== mySeq) {
          return;
        }

        setTotalCount(res.totalCount);
        setPage(res.page);
        if (res.pageSize > 0) {
          setPageSize(res.pageSize);
        }
        setWorks((prev) => {
          if (!append) return res.items;
          const existingIds = new Set(prev.map((w) => w.sourceWorkId));
          const deduped = res.items.filter(
            (w) => !existingIds.has(w.sourceWorkId)
          );
          return [...prev, ...deduped];
        });
      } catch (e) {
        if (reqSeqRef.current === mySeq) {
          onError(normalizeAppError(e));
        }
      } finally {
        if (reqSeqRef.current === mySeq) {
          setLoading(false);
        }
      }
    },
    [committedKeyword, order, sort, subtitleOnly, onError]
  );

  useEffect(() => {
    void fetchPage(1, false);
  }, [fetchPage]);

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

  const handleSearchSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    setCommittedKeyword(keyword);
  };

  const handleSelectCategory = (nextOrder: string) => {
    setOrder(nextOrder);
    setSort("desc");
  };

  const handleToggleNsfwMode = async () => {
    const nextVal = !nsfwEnabled;
    setNsfwEnabled(nextVal);
    try {
      await callCommand<boolean>("set_nsfw_enabled", { enabled: nextVal });
      void fetchPage(1, false);
    } catch (e) {
      onError(normalizeAppError(e));
    }
  };

  const handleOpenOnlineWork = async (idOrRj: string) => {
    if (!idOrRj.trim() || openingId) return;
    setContextMenu(null);
    setOpeningId(idOrRj);
    try {
      const detail = await callCommand<WorkDetailDto>("open_online_work", {
        workIdOrRj: idOrRj.trim(),
      });
      onOpenWorkDetail(detail.id);
    } catch (e) {
      onError(normalizeAppError(e));
    } finally {
      setOpeningId(null);
    }
  };

  const handleOpenDownloadModal = async (w: ProviderWork) => {
    if (downloadingId) return;
    setContextMenu(null);
    setDownloadingId(w.sourceWorkId);
    try {
      const detail = await callCommand<WorkDetailDto>("open_online_work", {
        workIdOrRj: w.sourceWorkId,
      });
      const prev = await callCommand<DownloadPreviewDto>(
        "prepare_work_download",
        {
          workId: detail.id,
          rjOrId: w.rjCode ?? w.sourceWorkId,
        }
      );
      setDlPreview(prev);
      const nextSel: Record<string, boolean> = {};
      for (const item of prev.items) {
        if (!item.alreadyDownloaded && !item.alreadyQueued) {
          nextSel[item.sourceId] = true;
        }
      }
      setDlSelectedIds(nextSel);
    } catch (e) {
      onError(normalizeAppError(e));
    } finally {
      setDownloadingId(null);
    }
  };

  const handleConfirmDownloadModal = async () => {
    if (!dlPreview) return;
    const sourceIds = Object.entries(dlSelectedIds)
      .filter(([, checked]) => checked)
      .map(([id]) => id);
    if (sourceIds.length === 0) return;
    try {
      await callCommand<DownloadTaskDto[]>("enqueue_downloads", {
        workId: dlPreview.workId,
        sourceIds,
      });
      setDlPreview(null);
      onOpenDownloads?.();
    } catch (e) {
      onError(normalizeAppError(e));
    }
  };

  const categories = [
    { key: "popular", label: "热门作品", icon: Flame },
    { key: "dl_count", label: "销量榜", icon: Trophy },
    { key: "rate_average_2dp", label: "好评榜", icon: Star },
    { key: "create_date", label: "最新收录", icon: Sparkles },
    { key: "release", label: "最新发售", icon: Calendar },
    { key: "review_count", label: "热议榜", icon: MessageSquare },
  ];

  const totalPages = Math.max(1, Math.ceil(totalCount / Math.max(1, pageSize)));

  return (
    <div className="page-container">
      {/* M3 Top App Bar */}
      <header className="top-app-bar" aria-label="在线浏览顶栏">
        <div className="top-bar-primary-row">
          <div className="top-bar-title-group">
            <h2 className="top-bar-title">在线浏览</h2>
            <span className="top-bar-count">共 {totalCount} 部作品</span>
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
            {committedKeyword && (
              <button
                type="button"
                className="btn btn-ghost btn-sm"
                onClick={() => {
                  setKeyword("");
                  setCommittedKeyword("");
                }}
              >
                清除搜索
              </button>
            )}
            <button
              type="button"
              className="btn-icon"
              onClick={() => void fetchPage(page, false)}
              aria-label="刷新在线作品列表"
              title="刷新"
            >
              <RefreshCw size={15} />
            </button>
          </div>
        </div>

        <div className="top-bar-controls-row">
          <form onSubmit={handleSearchSubmit} className="search-input-wrap">
            <Search size={15} className="text-muted" />
            <input
              type="text"
              className="input-text"
              placeholder="搜索关键词、RJ号、声优或标签..."
              value={keyword}
              onChange={(e) => setKeyword(e.target.value)}
            />
            <button type="submit" className="btn btn-primary btn-sm">
              搜索
            </button>
          </form>
        </div>
      </header>

      {/* M3 热门作品与榜单切换栏 */}
      <div className="online-category-bar">
        <div className="category-chips-group" role="tablist" aria-label="在线作品榜单分类">
          {categories.map((cat) => {
            const IconComp = cat.icon;
            const isActive = order === cat.key;
            return (
              <button
                key={cat.key}
                type="button"
                role="tab"
                aria-selected={isActive}
                className={`btn-chip ${isActive ? "active" : ""}`}
                onClick={() => handleSelectCategory(cat.key)}
              >
                <IconComp size={13} />
                <span>{cat.label}</span>
              </button>
            );
          })}
        </div>

        <div className="filter-controls-group">
          <button
            type="button"
            className={`btn-chip ${nsfwEnabled ? "active btn-chip-nsfw" : ""}`}
            aria-label="切换全年龄或NSFW模式"
            aria-pressed={nsfwEnabled}
            onClick={() => void handleToggleNsfwMode()}
            title={
              nsfwEnabled
                ? "当前已开启 NSFW 模式（展示包含 R18 在内的全部作品，点击切换为全年龄过滤）"
                : "当前为全年龄安全模式（已过滤 R18 作品并自动补齐整页，点击开启 NSFW 完整展示）"
            }
          >
            {nsfwEnabled ? <Eye size={13} /> : <EyeOff size={13} />}
            <span>{nsfwEnabled ? "NSFW 已开启" : "全年龄过滤"}</span>
          </button>

          <button
            type="button"
            className={`btn-chip ${subtitleOnly ? "active" : ""}`}
            onClick={() => setSubtitleOnly((v) => !v)}
          >
            <Subtitles size={13} />
            <span>仅带字幕</span>
          </button>

          {order !== "popular" && (
            <select
              className="input-select input-sm"
              aria-label="排序方向"
              value={sort}
              onChange={(e) => setSort(e.target.value)}
            >
              <option value="desc">降序</option>
              <option value="asc">升序</option>
            </select>
          )}
        </div>
      </div>

      {works.length === 0 && !loading ? (
        <div className="empty-state-box">
          <h3>未找到匹配的在线作品</h3>
        </div>
      ) : (
        <>
          <div className="work-grid online-work-grid">
            {works.slice(0, 12).map((w) => (
              <article
                key={w.sourceWorkId}
                className="work-card"
                onClick={() => void handleOpenOnlineWork(w.sourceWorkId)}
                onContextMenu={(e) => {
                  e.preventDefault();
                  e.stopPropagation();
                  const clampedX = Math.min(
                    e.clientX,
                    Math.max(16, window.innerWidth - 250)
                  );
                  const clampedY = Math.min(
                    e.clientY,
                    Math.max(16, window.innerHeight - 180)
                  );
                  setContextMenu({ work: w, x: clampedX, y: clampedY });
                }}
              >
                {w.coverUrl && (
                  <div className="work-card-cover-wrap">
                    <img
                      src={w.coverUrl}
                      alt={w.title}
                      className="work-card-cover"
                      loading="lazy"
                      referrerPolicy="no-referrer"
                      onError={(e) => {
                        (
                          e.currentTarget.parentElement as HTMLElement
                        ).style.display = "none";
                      }}
                    />
                    {(w.nsfw || w.hasSubtitle) && (
                      <div className="work-card-cover-badges">
                        {w.hasSubtitle && (
                          <span className="badge badge-downloaded">含字幕</span>
                        )}
                        {w.nsfw && (
                          <span className="badge badge-nsfw">R18</span>
                        )}
                      </div>
                    )}
                  </div>
                )}

                <div className="work-card-header">
                  <div className="work-badges">
                    {w.rjCode && (
                      <span className="badge badge-rj">{w.rjCode}</span>
                    )}
                    {typeof w.rateAverage2dp === "number" &&
                      w.rateAverage2dp > 0 && (
                        <span className="badge badge-warn">
                          ★ {w.rateAverage2dp.toFixed(2)}
                        </span>
                      )}
                    {typeof w.dlCount === "number" && w.dlCount > 0 && (
                      <span className="badge badge-local">
                        销量 {w.dlCount.toLocaleString()}
                      </span>
                    )}
                    {!w.coverUrl && w.hasSubtitle && (
                      <span className="badge badge-downloaded">含字幕</span>
                    )}
                    {!w.coverUrl && w.nsfw && (
                      <span className="badge badge-nsfw">R18</span>
                    )}
                  </div>
                  <ExternalLink size={14} className="text-muted" />
                </div>

                <h3 className="work-card-title">{w.title}</h3>

                <div className="work-card-meta">
                  {w.circleName && <span>社团: {w.circleName}</span>}
                  {w.voiceActors.length > 0 && (
                    <span className="cv-list">
                      CV:{" "}
                      {w.voiceActors.slice(0, 4).map((va) => (
                        <button
                          key={va}
                          type="button"
                          className="cv-pill cv-pill-btn"
                          onClick={(e) => {
                            e.stopPropagation();
                            setKeyword(va);
                            setCommittedKeyword(va);
                            setPage(1);
                          }}
                          title={`搜索声优: ${va}`}
                        >
                          {va}
                        </button>
                      ))}
                    </span>
                  )}
                </div>

                {w.tags.length > 0 && (
                  <div className="work-tags">
                    {w.tags.slice(0, 5).map((tag) => (
                      <button
                        key={tag}
                        type="button"
                        className="tag-pill tag-pill-btn"
                        onClick={(e) => {
                          e.stopPropagation();
                          setKeyword(tag);
                          setCommittedKeyword(tag);
                          setPage(1);
                        }}
                        title={`搜索标签: ${tag}`}
                      >
                        {tag}
                      </button>
                    ))}
                  </div>
                )}
              </article>
            ))}
          </div>

          {totalCount > 0 && (
            <div className="pagination-row">
              <button
                type="button"
                className="btn btn-secondary btn-sm"
                disabled={loading || page <= 1}
                onClick={() => void fetchPage(page - 1, false)}
              >
                上一页
              </button>
              <span className="tabular-nums text-muted" style={{ padding: "0 12px" }}>
                第 {page} / {totalPages} 页
              </span>
              <button
                type="button"
                className="btn btn-secondary btn-sm"
                disabled={loading || page >= totalPages}
                onClick={() => void fetchPage(page + 1, false)}
              >
                下一页
              </button>
            </div>
          )}
        </>
      )}

      {/* 在线作品卡片右键菜单 */}
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
            aria-label="在线作品操作菜单"
            style={{ left: `${contextMenu.x}px`, top: `${contextMenu.y}px` }}
          >
            <div className="m3-context-menu-header">
              <div className="m3-context-menu-title">
                {contextMenu.work.title}
              </div>
              <div className="m3-context-menu-sub">
                {contextMenu.work.rjCode ?? `ID: ${contextMenu.work.sourceWorkId}`}
              </div>
            </div>

            <button
              type="button"
              className="m3-menu-item"
              role="menuitem"
              onClick={() =>
                void handleOpenOnlineWork(contextMenu.work.sourceWorkId)
              }
            >
              <FolderTree size={15} />
              <span>打开作品详情</span>
            </button>

            <button
              type="button"
              className="m3-menu-item"
              role="menuitem"
              onClick={() =>
                void handleOpenDownloadModal(contextMenu.work)
              }
            >
              <Download size={15} />
              <span>创建下载任务…</span>
            </button>
          </div>
        </>
      )}

      {dlPreview && (
        <DownloadTaskModal
          preview={dlPreview}
          selectedIds={dlSelectedIds}
          onChangeSelectedIds={setDlSelectedIds}
          onConfirm={() => void handleConfirmDownloadModal()}
          onClose={() => setDlPreview(null)}
        />
      )}
    </div>
  );
};
