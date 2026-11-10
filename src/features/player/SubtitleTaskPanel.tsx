import { useEffect, useState } from "react";
import { SubtitleActionButtons, useSubtitleActions } from "./useSubtitleActions";
import { ErrorNotice } from "../../components/ErrorNotice";
import { Settings } from "lucide-react";
import { callCommand } from "../../lib/api";
import type { EditableSubtitleDocument } from "../../lib/recognition";
import { subtitleResultCounts, subtitleTaskStatus, subtitleTaskUnit } from "./subtitleTaskPresentation";

export function SubtitleTaskPanel({ versionId, trackTitle, onOpenSettings, onReviewCue }: {
  versionId?: string | null; trackTitle?: string | null; onOpenSettings?: () => void;
  onReviewCue?: (index: number) => void;
}) {
  const ids = versionId ? [versionId] : [];
  const actions = useSubtitleActions(ids);
  const { task, running, busy, error } = actions;
  const sameTrack = !!versionId && task?.versionId === versionId;
  const [result, setResult] = useState<ReturnType<typeof subtitleResultCounts> | null>(null);
  const [resultError, setResultError] = useState(false);
  const [refresh, setRefresh] = useState(0);
  const settled = sameTrack && ["completed", "failed", "cancelled"].includes(task?.state ?? "");
  // Read saved cue states once on settlement, never for every polling tick.
  useEffect(() => {
    let stale = false;
    setResult(null); setResultError(false);
    if (settled && versionId) void callCommand<EditableSubtitleDocument>("get_editable_subtitle", { versionId })
      .then(doc => { if (!stale && doc.versionId === versionId) setResult(subtitleResultCounts(doc)); })
      .catch(() => { if (!stale) setResultError(true); });
    return () => { stale = true; };
  }, [settled, versionId, task?.state, task?.completed, task?.total, actions.notice, refresh]);
  const visibleTask = task && !!task.state && (sameTrack || running);
  const review = result?.review ?? task?.reviewPending ?? 0;
  return <section className="subtitle-task-panel" aria-label="识别与翻译">
    <div className="subtitle-task-heading"><strong>识别与翻译</strong>
      {onOpenSettings && <button type="button" className="btn-icon" aria-label="模型与服务设置" title="模型与服务设置" onClick={onOpenSettings}><Settings size={18} /></button>}
    </div>
    <p className="subtitle-task-track">{versionId ? trackTitle || "当前音轨" : "请先选择音轨"}</p>
    <div className="subtitle-action-grid subtitle-action-grid-pair subtitle-actions-compact">
      <SubtitleActionButtons actions={actions} versionIds={ids} tiles />
    </div>
    {visibleTask && <div className="subtitle-task-status">
      {!sameTrack && <p className="settings-field-hint">正在处理另一音轨 · {task.versionId}</p>}
      {(task.tracksTotal ?? 0) > 1 && <p className="settings-field-hint">音轨已完成 {task.tracksCompleted ?? 0} / {task.tracksTotal}</p>}
      <p role="status">{subtitleTaskStatus(task)}</p>
      <p className="subtitle-task-phase">{task.phase}</p>
      {task.total > 0 && <p className="subtitle-task-count">已处理 {task.completed} / {task.total} {subtitleTaskUnit(task.phase)}</p>}
      {running && <progress aria-label={`${task.phase}进度（${subtitleTaskUnit(task.phase)}）`} max={task.total || 1} value={task.total ? task.completed : undefined} />}
      {settled && !result && !resultError && <p className="settings-field-hint">正在读取保存结果…</p>}
      {result && <div className="subtitle-task-results" aria-label="已读取的保存结果">
        <span>已保存译文 {result.translated} 句</span><span>待翻译 {result.pending} 句</span>
        <span className={result.failed ? "subtitle-result-failed" : undefined}>翻译失败 {result.failed} 句</span>
      </div>}
      {result && <button type="button" className="btn btn-ghost btn-sm" onClick={() => setRefresh(n => n + 1)}>刷新结果</button>}
      {review > 0 && <p className="settings-field-hint">{review} 句待核对，与翻译失败分开统计；不影响按现有原文继续翻译。</p>}
      {resultError && <p className="settings-field-hint">保存结果统计暂不可用。<button type="button" className="btn btn-ghost btn-sm" onClick={() => setRefresh(n => n + 1)}>重试统计</button></p>}
      {running ? <button type="button" className="btn btn-ghost btn-sm" disabled={busy || task.state === "cancelling"} onClick={() => void actions.cancel()}>停止任务</button>
        : <>
          {(task.state !== "completed" || task.completed < task.total || !!result?.failed || !!result?.pending) && <p className="settings-field-hint">点击上方原操作，选择「增量」继续处理；已保存内容会保留。</p>}
          {sameTrack && onReviewCue && <div className="subtitle-result-actions">
            {result?.firstFailed !== undefined && <button type="button" className="btn btn-secondary btn-sm" onClick={() => onReviewCue(result.firstFailed!)}>查看失败字幕</button>}
            {result?.firstReview !== undefined && <button type="button" className="btn btn-secondary btn-sm" onClick={() => onReviewCue(result.firstReview!)}>查看待核对</button>}
          </div>}
        </>}
    </div>}
    <ErrorNotice message={error || (visibleTask ? task.error : null)} />
    {actions.notice && (!visibleTask || !actions.notice.startsWith("已开始")) && <p role="status" className="settings-field-hint">{actions.notice}</p>}
    {actions.dialog}
  </section>;
}
