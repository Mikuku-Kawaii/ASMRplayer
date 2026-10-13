import React, { useEffect, useState, useCallback } from "react";
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

  const loadTasks = useCallback(async () => {
    setLoading(true);
    try {
      const list = await callCommand<DownloadTaskDto[]>("list_download_tasks");
      setTasks(list);
      const firstFailed = list.find((t) => t.status === "failed" && t.errorMessage);
      if (firstFailed && firstFailed.errorMessage) {
        onError({
          code: firstFailed.errorCode ?? "DOWNLOAD_FAILED",
          message: `《${firstFailed.trackTitle}》下载失败：${firstFailed.errorMessage}`,
          retryable: true,
          hint: `已重试 ${firstFailed.retryCount} 次，可点击「续传」继续下载`,
        });
      }
    } catch (e) {
      onError(normalizeAppError(e));
    } finally {
      setLoading(false);
    }
  }, [onError]);

  useEffect(() => {
    void loadTasks();
    let unlisten: (() => void) | null = null;
    void subscribeDownloadsUpdated((updated) => {
      setTasks(updated);
      const failed = updated.find((t) => t.status === "failed" && t.errorMessage);
      if (failed && failed.errorMessage) {
        onError({
          code: failed.errorCode ?? "DOWNLOAD_FAILED",
          message: `《${failed.trackTitle}》下载失败：${failed.errorMessage}`,
          retryable: true,
          hint: `已重试 ${failed.retryCount} 次，可点击「续传」继续下载`,
        });
      }
    }).then((fn) => {
      unlisten = fn;
    });
    return () => {
      if (unlisten) unlisten();
    };
  }, [loadTasks, onError]);

  const handlePause = async (taskId: string) => {
    try {
      const updated = await callCommand<DownloadTaskDto[]>(
        "pause_download_task",
        { taskId }
      );
      setTasks(updated);
    } catch (e) {
      onError(normalizeAppError(e));
    }
  };

  const handleResume = async (taskId: string) => {
    try {
      const updated = await callCommand<DownloadTaskDto[]>(
        "resume_download_task",
        { taskId }
      );
      setTasks(updated);
    } catch (e) {
      onError(normalizeAppError(e));
    }
  };

  const handleCancel = async (taskId: string) => {
    try {
      const updated = await callCommand<DownloadTaskDto[]>(
        "cancel_download_task",
        { taskId }
      );
      setTasks(updated);
    } catch (e) {
      onError(normalizeAppError(e));
    }
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

      {tasks.length === 0 && !loading ? (
        <div className="empty-state-box">
          <h3>暂无下载任务</h3>
        </div>
      ) : tasks.length > 0 ? (
        <table className="tracks-table">
          <thead>
            <tr>
              <th>音轨 / 所属作品</th>
              <th>保存相对路径</th>
              <th>状态与校验方式</th>
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
                        <span>{statusLabel(task.status)}</span>
                      </span>
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
                  <td>
                    <div>
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
                        task.status === "resolving") && (
                        <button
                          type="button"
                          className="btn btn-secondary btn-sm"
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
                          onClick={() => void handleResume(task.id)}
                          title="通过 HTTP Range 206 断点续传"
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
        </table>
      ) : null}
    </div>
  );
};
