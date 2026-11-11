import { useEffect, useState } from "react";
import { open, save } from "@tauri-apps/plugin-dialog";
import { ContentDialog } from "../../components/ContentDialog";
import { ErrorNotice } from "../../components/ErrorNotice";
import { Select } from "../../components/Select";
import { callCommand, formatDurationMs, normalizeAppError } from "../../lib/api";

interface ScriptPreview {
  revision: string;
  matched: number;
  rows: { line: number; text: string; startMs: number | null; endMs: number | null; score: number }[];
}
export function ScriptAlignmentDialog({ versionId, onClose }: { versionId: string; onClose: () => void }) {
  const [script, setScript] = useState("");
  const [preview, setPreview] = useState<ScriptPreview | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [exported, setExported] = useState("");
  const [files, setFiles] = useState<{ path: string; title: string; attachmentId?: string | null }[]>([]);
  const [loadingFiles, setLoadingFiles] = useState(true);
  const [loadedPath, setLoadedPath] = useState("");
  useEffect(() => {
    let active = true;
    void callCommand<{ path: string; title: string; attachmentId?: string | null }[]>("list_local_scripts", { versionId })
      .then(async result => {
        if (!active) return;
        setFiles(result);
        const file = result[0];
        if (result.length === 1 && file?.attachmentId) {
          const text = await callCommand<string>("read_online_script", { versionId, attachmentId: file.attachmentId });
          if (active) { setScript(text); setLoadedPath(file.path); }
        }
      })
      .catch(e => { if (active) setError(normalizeAppError(e).message); })
      .finally(() => { if (active) setLoadingFiles(false); });
    return () => { active = false; };
  }, [versionId]);
  async function loadFile(path: string) {
    const file = files.find(file => file.path === path);
    const text = file?.attachmentId
      ? await callCommand<string>("read_online_script", { versionId, attachmentId: file.attachmentId })
      : await callCommand<string>("read_local_script", { path });
    setScript(text); setLoadedPath(path); setPreview(null); setExported("");
  }
  async function run(action: () => Promise<void>) {
    if (busy) return;
    setBusy(true); setError("");
    try { await action(); } catch (e) { setError(normalizeAppError(e).message); } finally { setBusy(false); }
  }
  return <ContentDialog label="无时间戳台本" busy={busy} onClose={onClose}>
    <h3>无时间戳台本</h3>
<div className="dialog-body">
    <p className="muted">选择日文台本或按台词分行粘贴。先完成整轨日语识别，再用识别字幕的时间范围匹配；不确定的段落不分配时间。</p>
    <div className="script-file-row">
      <Select aria-label="作品台本" value={loadedPath} disabled={busy || loadingFiles || !files.length} placeholder={loadingFiles ? "查找作品台本…" : files.length ? "选择作品台本" : "未发现作品台本"} options={files.map(file => ({ value: file.path, label: file.attachmentId ? `在线 · ${file.title}` : file.title }))} onValueChange={path => void run(() => loadFile(path))} />
      <button type="button" className="btn btn-secondary btn-sm" disabled={busy || loadingFiles} onClick={() => void run(async () => {
        const path = await open({ title: "选择无时间戳台本", multiple: false, directory: false, filters: [{ name: "日文台本", extensions: ["txt", "md"] }] });
        if (typeof path === "string") await loadFile(path);
      })}>选择文件</button>
    </div>
    <p className="muted script-file-hint">{files.find(file => file.path === loadedPath)?.attachmentId ? `在线原文：${files.find(file => file.path === loadedPath)?.title}` : loadedPath || "自动查找作品的本地及在线 TXT / MD，优先列出台本目录；单份在线台本自动载入，多份可选择。"}</p>
    <label htmlFor="japanese-script">日文台本</label>
    <textarea id="japanese-script" className="input-text script-alignment-input" rows={7} maxLength={30000} disabled={busy || loadingFiles} value={script} onChange={e => { setScript(e.target.value); setPreview(null); setExported(""); }} placeholder="每行一句原文，不需要时间戳" />
    <ErrorNotice message={error} />
    {preview && <><p role="status">已匹配 {preview.matched} / {preview.rows.length} 行；未匹配 {preview.rows.length - preview.matched} 行不会导出。</p>
      <ol className="script-alignment-results" aria-label="台本对齐预览">{preview.rows.map(row => <li key={row.line}><span className="muted">{row.startMs === null ? "未匹配" : `${formatDurationMs(row.startMs)}–${formatDurationMs(row.endMs ?? row.startMs)} · 相似度 ${row.score}%`}</span><p>{row.text}</p></li>)}</ol>
    </>}
    {exported && <p role="status">已导出匹配段落。可使用此字幕，再点击翻译。</p>}
    </div>
<div className="dialog-actions">
      <button className="btn btn-ghost" type="button" disabled={busy} onClick={onClose}>关闭</button>
      <button className="btn btn-secondary" type="button" disabled={busy || !script.trim()} onClick={() => void run(async () => { setPreview(await callCommand<ScriptPreview>("preview_script_alignment",{versionId,script})); setExported(""); })}>预览对齐</button>
      {exported ? <button className="btn btn-primary" type="button" disabled={busy} onClick={() => void run(async () => { await callCommand("import_local_subtitle",{versionId,path:exported}); onClose(); })}>使用导出字幕</button>
        : <button className="btn btn-primary" type="button" disabled={busy || !preview?.matched} onClick={() => void run(async () => {
          if (!preview) return;
          const targetPath=await save({title:"导出已匹配台本",defaultPath:"aligned-script.srt",filters:[{name:"SRT 字幕",extensions:["srt"]}]});
          if (!targetPath) return;
          await callCommand("export_script_alignment",{versionId,script,revision:preview.revision,targetPath}); setExported(targetPath);
        })}>导出匹配段落</button>}
    </div>
  </ContentDialog>;
}
