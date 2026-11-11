import { ErrorNotice } from "../../components/ErrorNotice";
import { useCallback, useEffect, useRef, useState } from "react";
import { save } from "@tauri-apps/plugin-dialog";
import { ContentDialog } from "../../components/ContentDialog";
import { Select } from "../../components/Select";
import { callCommand, formatDurationMs, normalizeAppError } from "../../lib/api";
import type { EditableSubtitleDocument } from "../../lib/recognition";

export function useSubtitleDocumentEditor(versionId?: string | null) {
  const [document, setDocument] = useState<EditableSubtitleDocument | null>(null);
  const [index, setIndex] = useState<number | null>(null);
  const [original, setOriginal] = useState("");
  const [translation, setTranslation] = useState("");
  const [termSource, setTermSource] = useState("");
  const [termTarget, setTermTarget] = useState("");
  const [format, setFormat] = useState("srt");
  const [mode, setMode] = useState("bilingual");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const generation = useRef(0), locked = useRef(false);
  const invalidate = useCallback(() => { ++generation.current; }, []);
  useEffect(() => {
    ++generation.current; setDocument(null); setError(""); setNotice("");
    return invalidate;
  }, [versionId, invalidate]);
  async function run(action: (current: () => boolean) => Promise<void>) {
    if (locked.current) return;
    const request = generation.current;
    locked.current = true; setBusy(true); setError(""); setNotice("");
    const current = () => generation.current === request;
    try { await action(current); }
    catch (e) { if (current()) setError(normalizeAppError(e).message); }
    finally { locked.current = false; setBusy(false); }
  }
  function open(cueIndex: number | null) {
    if (!versionId) return;
    void run(async current => {
      const next = await callCommand<EditableSubtitleDocument>("get_editable_subtitle", { versionId });
      if (!current()) return;
      if (!next.cues.length) throw new Error("当前音轨没有可编辑或导出的字幕。");
      const cue = next.cues.find(c => c.index === cueIndex);
      if (cueIndex !== null && !cue) throw new Error("字幕已变化，请重新选择需要编辑的句子。");
      setIndex(cueIndex); setOriginal(cue?.original ?? ""); setTranslation(cue?.translation ?? ""); setDocument(next);
      setTermSource(""); setTermTarget("");
    });
  }
  const cue = document?.cues.find(c => c.index === index);
  function saveEdit(reset: boolean, undoModel = false, excludeNonDialogue = false) {
    if (!document || !cue) return;
    void run(async current => {
      await callCommand("edit_subtitle_cue", { edit: { versionId: document.versionId, sourceKey: document.sourceKey, revision: document.revision, index: cue.index, original, translation, reset, undoModel, ...(excludeNonDialogue ? {excludeNonDialogue:true} : {}) } });
      if (current()) { setDocument(null); setNotice(excludeNonDialogue ? "本句已标记为非台词，原始记录保留。" : undoModel ? "已撤销模型校对，人工修正保留。" : reset ? "已撤销本句修正。" : "字幕修正已保存。"); }
    });
  }
  function exportFile() {
    if (!document) return;
    void run(async current => {
      const targetPath = await save({ title: "导出字幕", defaultPath: `字幕-${mode}.${format}`, filters: [{ name: format.toUpperCase(), extensions: [format] }] });
      if (!targetPath || !current()) return;
      const result = await callCommand<{ exportedCues: number }>("export_subtitle_document", { versionId: document.versionId, revision: document.revision, format, mode, targetPath });
      if (current()) { setDocument(null); setNotice(`已导出 ${result.exportedCues} 句字幕。`); }
    });
  }
  const dialogueCues = document?.cues.filter(c => c.status !== "nonDialogue") ?? [];
  const translated = dialogueCues.filter(c => c.translation.trim()).length;
  const dirty = cue && (original !== cue.original || translation !== cue.translation);
  const statusLabels = { pendingTranslation: "待翻译", translationFailed: "翻译失败", audioReview: "待音频核对", noSpeech: "已确认无有效语音", nonDialogue: "非台词，已隐藏", complete: "已完成" };
  const dialog = document && <ContentDialog size={cue ? "wide" : "standard"} label={cue ? "编辑字幕" : "导出字幕"} busy={busy} onClose={() => setDocument(null)}>
    <div className="subtitle-document-dialog dialog-form">
      <h3>{cue ? "编辑字幕" : "导出字幕"}</h3>
<div className="dialog-body">
      {cue ? <>
        <p className="settings-field-hint subtitle-document-time">{formatDurationMs(cue.startMs)} – {formatDurationMs(cue.endMs)}</p>
        {cue.status && <p role="status" className="settings-field-hint">{statusLabels[cue.status]}{cue.audioNote ? `：${cue.audioNote}` : ""}</p>}
        {cue.audioComparison != null && <details><summary>查看音频复识别对照</summary><p>首次识别片段：{cue.rawOriginal || "未返回文字"}</p><p>带上下文对照：{cue.audioComparison || "未返回文字"}</p><p className="settings-field-hint">对照可能包含前后台词；结果一致也不代表已确认正确。</p></details>}
        {cue.review?.note && <p role="status" className="settings-field-hint">待核对：{cue.review.note}</p>}
        {cue.modelOriginal != null && <details><summary>查看识别与校对记录</summary><p>原始识别：{cue.rawOriginal}</p><p>模型校对：{cue.modelOriginal}</p>{cue.review && <p className="settings-field-hint">{cue.review.contextCount < cue.review.totalCount ? "分块" : "全文"}上下文：ID {cue.review.contextFirstId}–{cue.review.contextLastId}，{cue.review.contextCount}/{cue.review.totalCount} 句</p>}</details>}
        <label>原文<textarea className="input-text" rows={4} value={original} disabled={busy} onChange={e => setOriginal(e.target.value)} /></label>
        <label>译文<textarea className="input-text" rows={4} value={translation} disabled={busy} onChange={e => setTranslation(e.target.value)} /></label>
        <p className="settings-field-hint">修正保存在应用中，外部字幕文件不变。修改原文会清除本句自动译文，手工译文保留。</p>
        <details className="subtitle-glossary-editor"><summary>加入作品词库</summary>
          <p className="settings-field-hint">填写本句的人名或术语。词条用于后续翻译，不会自动保存本句修改。</p>
          <label>词条原文<input className="input-text" maxLength={100} disabled={busy} value={termSource} onChange={e => setTermSource(e.target.value)} /></label>
          <label>词条译名<input className="input-text" maxLength={100} disabled={busy} value={termTarget} onChange={e => setTermTarget(e.target.value)} /></label>
          <button type="button" className="btn btn-secondary" disabled={busy || !termSource.trim() || !termTarget.trim()} onClick={() => void run(async current => {
            await callCommand("add_subtitle_glossary_term", { versionId: document.versionId, source: termSource.trim(), target: termTarget.trim() });
            if (current()) { setNotice("词条已加入本作品词库。"); setTermSource(""); setTermTarget(""); window.dispatchEvent(new Event("subtitle-models-changed")); }
          })}>保存词条</button>
        </details>
      </> : <>
        <label>文件格式<Select aria-label="文件格式" value={format} disabled={busy} onValueChange={setFormat} options={[{value:"srt",label:"SRT"},{value:"vtt",label:"WebVTT"},{value:"txt",label:"TXT（含时间）"}]} /></label>
        <label>字幕内容<Select aria-label="字幕内容" value={mode} disabled={busy} onValueChange={setMode} options={[{value:"original",label:"原文"},{value:"translation",label:"译文"},{value:"bilingual",label:"双语"}]} /></label>
        <p className="settings-field-hint">已保存 {dialogueCues.length} 句台词、{translated} 句译文，{document.cues.filter(item => item.review?.note).length} 句待核对。导出包含手工修正，编码为 UTF-8。</p>
        {document.cues.some(item => item.status === "nonDialogue") && <details><summary>查看已隐藏的非台词</summary><div className="dialog-actions">{document.cues.filter(item => item.status === "nonDialogue").map(item => <button type="button" className="btn btn-secondary" key={item.index} onClick={() => { setIndex(item.index); setOriginal(item.original); setTranslation(item.translation); }}>{formatDurationMs(item.startMs)} · {item.original}</button>)}</div><p className="settings-field-hint">原始记录保留。若实际是说话，可打开并确认本句恢复显示。</p></details>}
        {document.cues.some(item => item.status === "audioReview") && <details><summary>待音频核对的片段</summary><div className="dialog-actions">{document.cues.filter(item => item.status === "audioReview").map(item => <button type="button" className="btn btn-secondary" key={item.index} onClick={() => { setIndex(item.index); setOriginal(item.original); setTranslation(item.translation); }}>{formatDurationMs(item.startMs)} · {item.original ? "核对原文" : "补充漏识别内容"}</button>)}</div><p className="settings-field-hint">这些片段保留原始结果，暂不自动翻译；请听音后修正并确认。</p></details>}
        {!document.recognitionComplete && <p role="status" className="settings-field-hint">识别尚未完成，只导出当前已保存的句子，不代表完整音轨。</p>}
        {mode !== "original" && translated < dialogueCues.length && <p className="settings-field-hint">{dialogueCues.length - translated} 句没有译文；{mode === "translation" ? "仅译文导出会跳过这些句子，保留其余句子的时间轴。" : "双语导出会为这些句子保留原文。"}</p>}
      </>}
      {error && <ErrorNotice message={error} />}
      {notice && <p role="status" className="settings-field-hint">{notice}</p>}
      </div>
<div className="dialog-actions">
        <button type="button" className="btn btn-ghost" disabled={busy} onClick={() => setDocument(null)}>取消</button>
        {cue ? <>
          {cue.canExcludeNonDialogue && cue.status !== "nonDialogue" && <button type="button" className="btn btn-ghost" disabled={busy} title="仅隐藏本句声音误识，保留原始记录供核对" onClick={() => saveEdit(false, false, true)}>标记为非台词</button>}
          {cue.modelOriginal != null && <button type="button" className="btn btn-secondary" disabled={busy} onClick={() => saveEdit(false, true)}>撤销模型校对</button>}
          {cue.edited && <button type="button" className="btn btn-secondary" disabled={busy} onClick={() => saveEdit(true)}>撤销本句修正</button>}
          <button type="button" className="btn btn-primary" disabled={busy || (!dirty && !cue.review?.note && cue.status !== "audioReview" && cue.status !== "nonDialogue") || !original.trim()} onClick={() => saveEdit(false)}>{busy ? "正在保存…" : (cue.review?.note || cue.status === "audioReview" || cue.status === "nonDialogue") && !dirty ? "确认本句" : "保存修改"}</button>
        </> : <button type="button" className="btn btn-primary" disabled={busy || !dialogueCues.length || (mode === "translation" && translated === 0)} onClick={exportFile}>{busy ? "正在导出…" : "选择位置并导出"}</button>}
      </div>
    </div>
  </ContentDialog>;
  return { openEdit: (id: number) => open(id), openExport: () => open(null), busy, dialog, feedback: <>{error && !document && <ErrorNotice message={error} />}{notice && !document && <p role="status" className="settings-field-hint">{notice}</p>}</> };
}
