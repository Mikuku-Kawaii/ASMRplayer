import { SubtitleActionButtons, useSubtitleActions } from "./useSubtitleActions";
import { ErrorNotice } from "../../components/ErrorNotice";
import { Settings } from "lucide-react";

export function SubtitleTaskPanel({ versionId, onOpenSettings }: {
  versionId?: string | null;
  onOpenSettings?: () => void;
}) {
  const ids = versionId ? [versionId] : [];
  const actions = useSubtitleActions(ids);
  const { task, running, busy, error } = actions;
  const sameTrack = task?.versionId === versionId;
  const status = task?.state === "completed" ? (task.automatic ? "当前预读已完成" : "字幕任务已完成") : task?.state === "cancelled" ? "已停止，可继续已保存的进度" : task?.state === "cancelling" ? "正在停止当前片段…" : task?.phase;
  return <section className="subtitle-task-panel" aria-label="识别与翻译">
    <div className="subtitle-task-heading">
      <strong>识别与翻译</strong>
      {onOpenSettings && <button type="button" className="btn-icon" aria-label="模型与服务设置" title="打开识别与翻译设置" onClick={onOpenSettings}><Settings size={18} aria-hidden="true" /></button>}
    </div>

    <div className="subtitle-action-grid subtitle-action-grid-pair">
      <SubtitleActionButtons actions={actions} versionIds={ids} tiles />
    </div>
    {running && <div className="recognition-actions">
      <button type="button" className="btn btn-ghost btn-sm" disabled={busy || task?.state === "cancelling"} onClick={() => void actions.cancel()}>取消任务</button>
    </div>}
    {(sameTrack || running) && task?.state && <div className="recognition-progress">
      {(task.tracksTotal ?? 0) > 1 && <p>目录进度：{task.tracksCompleted} / {task.tracksTotal} 条音轨</p>}
      <p role="status">{!sameTrack && "另一音轨："}{status}{task.total > 0 && ` · ${task.completed} / ${task.total}`}</p>
      {(task.reviewPending ?? 0) > 0 && <p role="status" className="settings-field-hint">{task.reviewPending} 句待核对；打开字幕编辑可查看原因、确认或撤销模型校对。</p>}
      {running && <progress aria-label="字幕任务进度" max={task.total || 1} value={task.total ? task.completed : undefined} />}
      {(task.state === "failed" || task.state === "cancelled") && <p className="settings-field-hint">再次点击原操作即可继续。后台任务不会切换播放；译文按句保存。</p>}
    </div>}
    <ErrorNotice message={error || (sameTrack ? task?.error : null)} />
    {actions.notice && <p role="status" className="settings-field-hint">{actions.notice}</p>}
    {actions.dialog}
  </section>;
}
