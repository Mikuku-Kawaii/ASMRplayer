import React, { useEffect, useState, useCallback, useRef, useId } from "react";
import {
  Pause,
  Play,
  XCircle,
  RefreshCw,
  CheckCircle2,
  AlertTriangle,
  ShieldCheck,
  ListX,
  Settings2,
  FolderOpen,
  MoreHorizontal,
  CloudDownload,
  LoaderCircle,
} from "lucide-react";
import {
  DownloadTaskDto,
  callCommand,
  subscribeDownloadsUpdated,
  formatBytes,
  normalizeAppError,
  AppError,
} from "../../lib/api";

import { Select } from "../../components/Select";
import { ContextMenu } from "../../components/ContextMenu";

interface DownloadsViewProps {
  embedded?: boolean;
  onSelectWork: (workId: string) => void;
  onError: (err: AppError) => void;
  onOpenSettings: () => void;
}
type BulkAction = "resume" | "pause" | "cancel" | "clearCompleted";
const bulkCommands: Record<BulkAction, string> = {
  resume: "resume_all_download_tasks", pause: "pause_all_download_tasks", cancel: "cancel_all_download_tasks",
  clearCompleted: "clear_completed_download_tasks",
};

export const DownloadsView: React.FC<DownloadsViewProps> = ({
  embedded = false,
  onSelectWork,
  onError,
  onOpenSettings,
}) => {
  const [tasks, setTasks] = useState<DownloadTaskDto[]>([]);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState("");
  const [openingDirectory, setOpeningDirectory] = useState(false);
  const directoryOpening = useRef(false);
  const [moreMenu, setMoreMenu] = useState<{ x: number; y: number } | null>(null);
  const moreTrigger = useRef<HTMLButtonElement>(null);
  const moreMenuId = useId();
  const request = useRef(0);
  const mounted = useRef(false);

  const loadTasks = useCallback(async () => {
    const id = ++request.current;
    setLoading(true);
    setLoadError("");
    try {
      const list = await callCommand<DownloadTaskDto[]>("list_download_tasks");
      if (!mounted.current || id !== request.current) return;
      setTasks(list);
      const firstFailed = list.find((t) => t.status === "failed" && !t.nextRetryAt && t.errorMessage);
      if (firstFailed && firstFailed.errorMessage) {
        onError({
          code: firstFailed.errorCode ?? "DOWNLOAD_FAILED",
          message: `《${firstFailed.trackTitle}》下载失败：${firstFailed.errorMessage}`,
          retryable: true,
          hint: `已重试 ${firstFailed.retryCount} 次，可点击「续传」继续下载`,
        });
      }
    } catch (e) {
      if (mounted.current && id === request.current) {
        const error = normalizeAppError(e);
        setLoadError(error.message);
        onError(error);
      }
    } finally {
      if (mounted.current && id === request.current) setLoading(false);
    }
  }, [onError]);

  useEffect(() => {
    let disposed = false;
    mounted.current = true;
    void loadTasks();
    let unlisten: (() => void) | null = null;
    void subscribeDownloadsUpdated((updated) => {
      if (disposed) return;
      request.current++;
      setLoading(false);
      setLoadError("");
      setTasks(updated);
      const failed = updated.find((t) => t.status === "failed" && !t.nextRetryAt && t.errorMessage);
      if (failed && failed.errorMessage) {
        onError({
          code: failed.errorCode ?? "DOWNLOAD_FAILED",
          message: `《${failed.trackTitle}》下载失败：${failed.errorMessage}`,
          retryable: true,
          hint: `已重试 ${failed.retryCount} 次，可点击「续传」继续下载`,
        });
      }
    }).then((fn) => {
      if (disposed) fn(); else unlisten = fn;
    }).catch(e => { if (!disposed) onError(normalizeAppError(e)); });
    return () => {
      disposed = true;
      mounted.current = false;
      unlisten?.();
    };
  }, [loadTasks, onError]);

  const [pending,setPending]=useState<string[]>([]);
  const active=useRef(new Set<string>());
  const [bulkPending,setBulkPending]=useState<BulkAction | null>(null);
  const bulkActive=useRef(false);
  const mutate=async(cmd:string,taskId:string,extra:Record<string,unknown>={})=>{
    if(bulkActive.current || active.current.has(taskId))return;
    active.current.add(taskId);setPending([...active.current]);
    setLoading(false);
    const id=++request.current;
    try {
      const updated=await callCommand<DownloadTaskDto[]>(cmd,{taskId,...extra});
      if(mounted.current&&id===request.current)setTasks(updated);
    }catch(e){if(mounted.current)onError(normalizeAppError(e));}
    finally{active.current.delete(taskId);if(mounted.current)setPending([...active.current]);}
  };
  const handlePause=(id:string)=>mutate("pause_download_task",id);
  const handleResume=(id:string)=>mutate("resume_download_task",id);
  const handleCancel=(id:string)=>mutate("cancel_download_task",id);
  const mutateAll=async(action:BulkAction)=>{
    if(bulkActive.current || active.current.size)return;
    bulkActive.current=true;setBulkPending(action);
    const id=++request.current;
    try {
      const updated=await callCommand<DownloadTaskDto[]>(bulkCommands[action]);
      if(mounted.current&&id===request.current)setTasks(updated);
    }catch(e){
      if(mounted.current){onError(normalizeAppError(e));await loadTasks();}
    }finally{bulkActive.current=false;if(mounted.current)setBulkPending(null);}
  };
  const bulkDisabled=loading || bulkPending!==null || pending.length>0;
  const canResume=tasks.some(task=>["paused","failed"].includes(task.status));
  const canPause=tasks.some(task=>["queued","resolving","downloading","verifying","failed"].includes(task.status));
  const canCancel=tasks.some(task=>!["completed","canceled"].includes(task.status));
  const canClearCompleted=tasks.some(task=>task.status==="completed");

  const openDownloadDirectory = async () => {
    if (directoryOpening.current) return;
    directoryOpening.current = true;
    setOpeningDirectory(true);
    try {
      await callCommand("open_download_directory");
    } catch (error) {
      if (mounted.current) onError(normalizeAppError(error));
    } finally {
      directoryOpening.current = false;
      if (mounted.current) setOpeningDirectory(false);
    }
  };

  const toggleMoreMenu = () => {
    const rect = moreTrigger.current?.getBoundingClientRect();
    if (rect) setMoreMenu(current => current ? null : { x: rect.right, y: rect.bottom + 6 });
  };

  const statusLabel = (s: string) => {
    switch (s) {
      case "queued":
        return "排队中";
      case "resolving":
        return "解析地址";
      case "downloading":
        return "下载中";
      case "verifying":
        return "校验中";
      case "completed":
        return "已完成";
      case "paused":
        return "已暂停";
      case "failed":
        return "下载失败";
      case "canceled":
        return "已取消";
      default:
        return s;
    }
  };

  return (
    <div className={embedded ? "downloads-page task-downloads" : "page-container downloads-page"}>
      <div className="download-panel">
      <header className="downloads-top-bar" aria-label="下载工具栏">
          <div className="downloads-heading">
            {embedded ? <h3>下载队列</h3> : <h2>离线下载</h2>}
            <span className="downloads-count tabular-nums">{tasks.length ? `${tasks.length} 个文件` : loading ? "正在读取" : loadError ? "未读取" : "0 个文件"}</span>
          </div>
          {tasks.length > 0 && <div className="downloads-toolbar-actions" role="group" aria-label="常用下载操作" aria-busy={bulkPending!==null}>
            <button type="button" className="btn btn-primary btn-sm" disabled={bulkDisabled || !canResume} onClick={()=>void mutateAll("resume")}
              title="继续所有已暂停或失败的任务，遵守下载时段与并发限制">
              <Play size={14} aria-hidden="true"/><span className="ui-icon-label">全部继续</span>
            </button>
            <button type="button" className="btn btn-secondary btn-sm" disabled={bulkDisabled || !canPause} onClick={()=>void mutateAll("pause")}
              title="暂停所有未完成任务，保留下载进度和 .part 临时文件">
              <Pause size={14} aria-hidden="true"/><span className="ui-icon-label">全部暂停</span>
            </button>
          </div>}
          <div className="downloads-toolbar-tools" role="group" aria-label="下载工具">
            <button type="button" className="btn-icon" disabled={openingDirectory} aria-busy={openingDirectory}
              onClick={() => void openDownloadDirectory()} aria-label="在资源管理器中打开" title="在资源管理器中打开下载目录">
              <FolderOpen size={18} aria-hidden="true" />
            </button>
            <button type="button" className="btn-icon" disabled={bulkDisabled} onClick={() => void loadTasks()} aria-label="刷新状态" title="刷新下载状态">
              <RefreshCw size={18} aria-hidden="true" />
            </button>
            <button ref={moreTrigger} type="button" className="btn-icon" onClick={toggleMoreMenu} aria-label="更多下载操作" title="更多下载操作"
              aria-haspopup="menu" aria-expanded={moreMenu!==null} aria-controls={moreMenu ? moreMenuId : undefined}
              onKeyDown={event => { if (event.key === "ArrowDown" && !moreMenu) { event.preventDefault(); event.stopPropagation(); toggleMoreMenu(); } }}>
              <MoreHorizontal size={18} aria-hidden="true" />
            </button>
          </div>
      </header>

      {moreMenu && <ContextMenu {...moreMenu} id={moreMenuId} trigger={moreTrigger.current ?? undefined} label="下载更多操作" onClose={() => setMoreMenu(null)}>
        <button type="button" role="menuitem" className="m3-menu-item" disabled={bulkDisabled || !canClearCompleted}
          title="仅清除已完成记录，保留下载文件和音频库来源" onClick={() => { setMoreMenu(null); void mutateAll("clearCompleted"); }}>
          <ListX size={18} aria-hidden="true" /><span className="ui-icon-label">{bulkPending === "clearCompleted" ? "正在清除…" : "清除已完成任务"}</span>
        </button>
        <button type="button" role="menuitem" className="m3-menu-item" onClick={() => { setMoreMenu(null); onOpenSettings(); }}>
          <Settings2 size={18} aria-hidden="true" /><span className="ui-icon-label">下载设置</span>
        </button>
        <div className="m3-menu-divider" role="separator" />
        <button type="button" role="menuitem" className="m3-menu-item danger" disabled={bulkDisabled || !canCancel}
          title="取消所有未完成任务并清理 .part 临时文件，保留已完成文件" onClick={() => { setMoreMenu(null); void mutateAll("cancel"); }}>
          <XCircle size={18} aria-hidden="true" /><span className="ui-icon-label">全部取消</span>
        </button>
      </ContextMenu>}

      {tasks.length === 0 ? (
        <div className={`downloads-empty${loadError && !loading ? " is-error" : ""}`} role="status">
          <span className={`downloads-empty-icon${loading ? " is-loading" : ""}`} aria-hidden="true">
            {loading ? <LoaderCircle size={24}/> : loadError ? <AlertTriangle size={24}/> : <CloudDownload size={24}/>}
          </span>
          <div className="downloads-empty-copy">
            <h3>{loading ? "正在读取下载任务" : loadError ? "下载任务读取失败" : "暂无下载任务"}</h3>
            <p>{loading ? "稍等片刻，正在获取最新状态。" : loadError || "在作品详情中选择文件，即可添加下载。"}</p>
            {!loading && loadError && <button className="btn btn-secondary btn-sm" onClick={()=>void loadTasks()}>重新读取</button>}
          </div>
        </div>
      ) : tasks.length > 0 ? (
        <div className="downloads-table-scroll" role="region" aria-label="下载任务列表" tabIndex={0}><table className="tracks-table">
          <thead>
            <tr>
              <th>音轨 / 所属作品</th>
              <th>保存相对路径</th>
              <th>状态与校验方式</th>
              <th>优先级</th>
              <th>进度 / 速度</th>
              <th>操作</th>
            </tr>
          </thead>
          <tbody>
            {tasks.map((task) => {
              const pct =
                task.totalBytes && task.totalBytes > 0
                  ? Math.min(
                      100,
                      Math.round((task.downloadedBytes / task.totalBytes) * 100)
                    )
                  : null;
              return (
                <tr key={task.id} className="track-row">
                  <td>
                    <div className="track-title-cell">
                      <span className="track-main-name">{task.trackTitle}</span>
                      <button
                        type="button"
                        className="link-btn"
                        onClick={() => onSelectWork(task.workId)}
                      >
                        {task.workTitle}
                      </button>
                    </div>
                  </td>
                  <td>
                    <code className="code-inline">{task.targetRelativePath}</code>
                  </td>
                  <td>
                    <div className="work-badges">
                      <span
                        className={`badge status-${task.status}`}
                        style={
                          task.status === "failed" && task.errorMessage
                            ? { cursor: "pointer" }
                            : undefined
                        }
                        title={
                          task.status === "failed" && task.errorMessage
                            ? `点击查看错误详情：[${task.errorCode ?? "ERROR"}] ${
                                task.errorMessage
                              }`
                            : undefined
                        }
                        onClick={() => {
                          if (task.status === "failed" && task.errorMessage) {
                            onError({
                              code: task.errorCode ?? "DOWNLOAD_FAILED",
                              message: `《${task.trackTitle}》下载失败：${task.errorMessage}`,
                              retryable: true,
                              hint: `已重试 ${task.retryCount} 次，可点击「续传」继续下载`,
                            });
                          }
                        }}
                      >
                        {task.status === "completed" && <CheckCircle2 size={11} />}
                        {task.status === "failed" && <AlertTriangle size={11} />}
                        <span>{task.status === "failed" && task.nextRetryAt ? "等待网络重试" : statusLabel(task.status)}</span>
                      </span>
                      {task.nextRetryAt && <span className="download-retry-hint" title={task.errorMessage ?? undefined}>网络重试最早时间：{new Date(task.nextRetryAt).toLocaleTimeString()}；是否自动继续取决于策略</span>}
                      <span
                        className="badge badge-local"
                        title={
                          task.integrityMode === "remote_sha256"
                            ? "已通过远端 SHA-256 摘要验证"
                            : "远端未提供内容哈希，已执行字节长度与非 HTML 媒体头完整性验证"
                        }
                      >
                        <ShieldCheck size={11} />
                        <span>
                          {task.integrityMode === "remote_sha256"
                            ? "远端哈希校验"
                            : "长度+媒体头校验"}
                        </span>
                      </span>
                    </div>
                  </td>
                  <td><Select size="compact" aria-label={`${task.trackTitle} 下载优先级`} value={String(task.priority ?? 0)} disabled={bulkPending!==null || pending.includes(task.id)||["completed","canceled"].includes(task.status)} options={[{value:"1",label:"优先"},{value:"0",label:"普通"},{value:"-1",label:"稍后"}]} onValueChange={value=>void mutate("set_download_priority",task.id,{priority:Number(value)})}/></td>
                  <td>
                    <div className="tabular-nums">
                      {formatBytes(task.downloadedBytes)} /{" "}
                      {task.totalBytes
                        ? formatBytes(task.totalBytes)
                        : "未知总长度"}
                      {pct !== null ? ` (${pct}%)` : ""}
                    </div>
                    {task.status === "downloading" && task.speedBps > 0 && (
                      <div className="text-accent">
                        {formatBytes(task.speedBps)}/s
                      </div>
                    )}
                  </td>
                  <td>
                    <div className="track-actions">
                      {(task.status === "downloading" ||
                        task.status === "queued" ||
                        task.status === "resolving" || (task.status === "failed" && !!task.nextRetryAt)) && (
                        <button
                          type="button"
                          className="btn btn-secondary btn-sm"
                          disabled={bulkPending!==null || pending.includes(task.id)}
                          onClick={() => void handlePause(task.id)}
                          title="暂停下载并保留同目录 .part 分片"
                        >
                          <Pause size={13} />
                          <span>暂停</span>
                        </button>
                      )}
                      {(task.status === "paused" || task.status === "failed") && (
                        <button
                          type="button"
                          className="btn btn-primary btn-sm"
                          disabled={bulkPending!==null || pending.includes(task.id)}
                          onClick={() => void handleResume(task.id)}
                          title="继续下载，仍遵守当前下载时段与并发限制"
                        >
                          <Play size={13} />
                          <span>继续</span>
                        </button>
                      )}
                      {task.status !== "completed" &&
                        task.status !== "canceled" && (
                          <button
                            type="button"
                            className="btn btn-ghost btn-sm"
                            disabled={bulkPending!==null || pending.includes(task.id)}
                          onClick={() => void handleCancel(task.id)}
                            title="取消下载任务并自动清理未完成的 .part 临时文件"
                          >
                            <XCircle size={13} />
                            <span>取消下载</span>
                          </button>
                        )}
                    </div>
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table></div>
      ) : null}
      </div>
    </div>
  );
};
