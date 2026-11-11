import { ErrorNotice } from "../../components/ErrorNotice";
import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { AudioLines, Languages } from "lucide-react";
import { ContentDialog } from "../../components/ContentDialog";
import { callCommand, normalizeAppError } from "../../lib/api";
import type { SubtitleTaskDto } from "../../lib/recognition";

interface Readiness { versionId: string; canRecognize: boolean; canTranslate: boolean; recognitionError?: string | null; translationError?: string | null }
export function useSubtitleActions(versionIds: string[]) {
  const key = JSON.stringify([...new Set(versionIds)]);
  const [task, setTask] = useState<SubtitleTaskDto | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [confirmation, setConfirmation] = useState<{ ids: string[]; translate: boolean; count: number; chinese: number; deletable: string[] } | null>(null);
  const [notice, setNotice] = useState("");
  const revision = useRef(0), locked = useRef(false);
  const taskRequest = useRef(0);
  const invalidate = useCallback(() => { ++revision.current; }, []);
  useEffect(() => {
    const current = ++revision.current;
    let polling = false;
    setError(""); setNotice(""); setConfirmation(null);
    const refreshTask = async () => {
      if (polling || locked.current) return;
      polling = true;
      const requestId = ++taskRequest.current;
      try {
        const value = await callCommand<SubtitleTaskDto>("get_subtitle_task");
        if (current === revision.current && requestId === taskRequest.current) setTask(value);
      } catch { /* Explicit actions surface command errors. */ }
      finally { polling = false; }
    };
    void refreshTask();
    const timer = window.setInterval(() => void refreshTask(), 1000);
    return () => { invalidate(); window.clearInterval(timer); };
  }, [key, invalidate]);
  const running = task?.state === "running" || task?.state === "cancelling";
  const can = (ids: string[], _translate: boolean) => !busy && !running && !confirmation && ids.length > 0;
  async function request(ids: string[], translate: boolean, restart?: boolean, confirmedChinese = false) {
    if (locked.current || running || ids.length === 0) return;
    const current = revision.current;
    locked.current = true; ++taskRequest.current; setBusy(true); setError(""); setNotice("");
    try {
      if (restart === undefined) {
        const info = await callCommand<{ existing: string[]; chinese: string[]; deletableRecognition?: string[]; deletableTranslation?: string[] }>("inspect_subtitle_tasks", { versionIds: ids });
        if (current !== revision.current) return;
        if (info.existing.length || (translate && info.chinese.length)) {
          setConfirmation({ ids: [...ids], translate, count: info.existing.length, chinese: info.chinese.length,
            deletable: (translate ? info.deletableTranslation : info.deletableRecognition) ?? [] }); return;
        }
      }
      const readiness = await callCommand<Readiness[]>("get_subtitle_readiness", { versionIds: ids, translate });
      if (current !== revision.current) return;
      const unavailable = ids.map(id => readiness.find(item => item.versionId === id)).find(item => !item || !(translate ? item.canTranslate : item.canRecognize));
      if (!ids.every(id => readiness.some(item => item.versionId === id && (translate ? item.canTranslate : item.canRecognize)))) {
        setError((translate ? unavailable?.translationError : unavailable?.recognitionError) || (translate
          ? "翻译条件未就绪：请在设置 → 识别与翻译中检查翻译模型或 DeepSeek 配置；无原文时还需识别模型和可用音源。模型需手动下载。"
          : "识别条件未就绪：请在设置 → 识别与翻译中检查识别模型和运行库，并确认音源可用。模型需手动下载。"));
        return;
      }
      const next = await callCommand<SubtitleTaskDto>("start_subtitle_tasks", { versionIds: ids, translate, confirmedChinese, restart: restart ?? false });
      if (current === revision.current) { setTask(next); setConfirmation(null); setNotice(`已开始${translate ? "翻译" : "识别"}${ids.length > 1 ? ` ${ids.length} 条音轨` : ""}，可在字幕台本查看进度`); }
    } catch (e) { if (current === revision.current) setError(normalizeAppError(e).message); }
    finally { locked.current = false; setBusy(false); }
  }
  async function deleteExisting() {
    if (!confirmation?.deletable.length || locked.current || running) return;
    const current = revision.current;
    const { deletable, translate } = confirmation;
    locked.current = true; ++taskRequest.current; setBusy(true); setError("");
    try {
      await callCommand("delete_generated_subtitles", { versionIds: deletable, translate });
      if (current !== revision.current) return;
      setConfirmation(null);
      setNotice(`已删除 ${deletable.length} 条音轨的生成${translate ? "译文" : "识别结果"}`);
      const next = await callCommand<SubtitleTaskDto>("get_subtitle_task");
      if (current === revision.current) setTask(next);
    } catch (e) { if (current === revision.current) setError(normalizeAppError(e).message); }
    finally { locked.current = false; setBusy(false); }
  }
  async function cancel() {
    if (locked.current) return;
    locked.current = true; ++taskRequest.current; setBusy(true);
    try { await callCommand("cancel_subtitle_task"); setTask(await callCommand<SubtitleTaskDto>("get_subtitle_task")); }
    catch (e) { setError(normalizeAppError(e).message); }
    finally { locked.current = false; setBusy(false); }
  }
  const verb = confirmation?.translate ? "翻译" : "识别";
  const dialog = confirmation && <ContentDialog label={`选择${verb}方式`} busy={busy} onClose={() => setConfirmation(null)}>
    <h3>选择{verb}方式</h3>
<div className="dialog-body">
    <ErrorNotice message={error} />
    <p>{confirmation.count} 条音轨已有字幕或处理进度。</p>
    {confirmation.translate && confirmation.chinese > 0 && <p>其中 {confirmation.chinese} 条字幕文件已含中文。</p>}
    <p>{confirmation.translate ? "增量翻译只补齐缺失译文；重新翻译会重算自动译文，保留手工译文和原时间轴。" : "开始识别会清除本音轨的自动译文。增量识别继续已有进度；重新识别会清除旧识别进度和原文修正，从头生成。"}外部字幕文件保留。</p>
    </div>
<div className="dialog-actions">
      {confirmation.deletable.length > 0 && <button type="button" className="btn btn-danger" disabled={busy} title={`仅删除 ${confirmation.deletable.length} 条音轨的生成${confirmation.translate ? "译文" : "识别结果"}，保留字幕文件`} onClick={() => void deleteExisting()}>删除已有</button>}
      <button type="button" className="btn btn-ghost" disabled={busy} onClick={() => setConfirmation(null)}>取消</button>
      <button type="button" className="btn btn-secondary" disabled={busy} onClick={() => void request(confirmation.ids, confirmation.translate, true, confirmation.chinese > 0)}>重新{verb}</button>
      <button type="button" className="btn btn-primary" disabled={busy} onClick={() => void request(confirmation.ids, confirmation.translate, false, confirmation.chinese > 0)}>增量{verb}</button>
    </div>
  </ContentDialog>;
  return { can, request, cancel, busy, running, task, error, notice, dialog };
}

export function SubtitleActionButtons({ actions, versionIds, tiles = false }: { actions: ReturnType<typeof useSubtitleActions>; versionIds: string[]; tiles?: boolean }) {
  return <>
    <button type="button" className={`btn btn-secondary${tiles ? "" : " btn-sm"}`} disabled={!actions.can(versionIds, false)} title="点击检查识别模型并识别所选音轨" onClick={() => void actions.request(versionIds, false)}>{tiles && <AudioLines size={30} aria-hidden="true" />}<span className="ui-icon-label">{tiles ? "识别字幕" : "识别"}</span></button>
    <button type="button" className={`btn btn-secondary${tiles ? "" : " btn-sm"}`} disabled={!actions.can(versionIds, true)} title="点击检查翻译服务并翻译所选音轨" onClick={() => void actions.request(versionIds, true)}>{tiles && <Languages size={30} aria-hidden="true" />}<span className="ui-icon-label">{tiles ? "翻译字幕" : "翻译"}</span></button>
  </>;
}

export function SubtitleActions({ versionIds, tiles = false, children }: { versionIds: string[]; tiles?: boolean; children?: ReactNode }) {
  const actions = useSubtitleActions(versionIds);
  return <div className={`subtitle-inline-actions${tiles ? " is-tile-layout" : ""}`}><div className={tiles ? "subtitle-action-grid" : "recognition-actions"}><SubtitleActionButtons actions={actions} versionIds={versionIds} tiles={tiles} />{children}</div>{actions.error && <ErrorNotice message={actions.error} />}{actions.notice && <p role="status" className="settings-field-hint">{actions.notice}</p>}{actions.dialog}</div>;
}
