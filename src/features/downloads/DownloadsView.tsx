import React, { useEffect, useState, useCallback, useRef } from "react";
import {
  Pause,
  Play,
  XCircle,
  RefreshCw,
  CheckCircle2,
  AlertTriangle,
  ShieldCheck,
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
import { DownloadPolicyPanel } from "./DownloadPolicyPanel";

interface DownloadsViewProps {
  onSelectWork: (workId: string) => void;
  onError: (err: AppError) => void;
}

export const DownloadsView: React.FC<DownloadsViewProps> = ({
  onSelectWork,
  onError,
}) => {
  const [tasks, setTasks] = useState<DownloadTaskDto[]>([]);
  const [loading, setLoading] = useState(false);
  const request = useRef(0);
  const mounted = useRef(false);

  const loadTasks = useCallback(async () => {
    const id = ++request.current;
    setLoading(true);
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
      if (mounted.current && id === request.current) onError(normalizeAppError(e));
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
  const mutate=async(cmd:string,taskId:string,extra:Record<string,unknown>={})=>{
    if(active.current.has(taskId))return;
    active.current.add(taskId);setPending([...active.current]);
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
    <div className="page-container">
      <header className="top-app-bar">
        <div className="top-app-bar-main">
          <div className="top-app-bar-title">
            <h2>离线下载任务管理</h2>
          </div>
          <div className="action-group">
            <button
              type="button"
              className="btn btn-secondary btn-sm"
              onClick={() => void loadTasks()}
            >
              <RefreshCw size={14} />
              <span>刷新状态</span>
            </button>
          </div>
        </div>
      </header>

      <DownloadPolicyPanel />
      {tasks.length === 0 && !loading ? (
        <div className="empty-state-box">
          <h3>暂无下载任务</h3>
          <p>在作品详情中选择音轨并加入下载队列。</p>
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
                  <td><Select size="compact" aria-label={`${task.trackTitle} 下载优先级`} value={String(task.priority ?? 0)} disabled={pending.includes(task.id)||["completed","canceled"].includes(task.status)} options={[{value:"1",label:"优先"},{value:"0",label:"普通"},{value:"-1",label:"稍后"}]} onValueChange={value=>void mutate("set_download_priority",task.id,{priority:Number(value)})}/></td>
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
                          disabled={pending.includes(task.id)}
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
                          disabled={pending.includes(task.id)}
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
                            disabled={pending.includes(task.id)}
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
  );
};
