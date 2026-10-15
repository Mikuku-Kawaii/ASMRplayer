import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { save } from "@tauri-apps/plugin-dialog";
import { Download, FileSearch, X } from "lucide-react";
import { callCommand, formatBytes, normalizeAppError } from "../../lib/api";

export interface DiagnosticPreview { token: string; generatedAt: string; bytes: number; sha256: string; content: string }

export function DiagnosticSettings() {
  const [preview, setPreview] = useState<DiagnosticPreview | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const locked = useRef(false), mounted = useRef(false);
  useEffect(() => { mounted.current = true; return () => { mounted.current = false; }; }, []);
  async function run(action: () => Promise<void>) {
    if (locked.current) return;
    locked.current = true; setBusy(true); setError(""); setNotice("");
    try { await action(); }
    catch (e) { if (mounted.current) setError(normalizeAppError(e).message); }
    finally { locked.current = false; if (mounted.current) setBusy(false); }
  }
  async function generate() {
    await run(async () => {
      const result = await callCommand<DiagnosticPreview>("preview_diagnostic_bundle");
      if (mounted.current) setPreview(result);
    });
  }
  async function exportPreview() {
    if (!preview) return;
    await run(async () => {
      const path = await save({ title: "保存诊断包", defaultPath: `asmr-diagnostic-${new Date().toISOString().replace(/[:.]/g, "-")}.json`, filters: [{ name: "JSON", extensions: ["json"] }] });
      if (!path || !mounted.current) return;
      await callCommand("export_diagnostic_bundle", { token: preview.token, targetPath: path });
      if (mounted.current) { setPreview(null); setNotice("诊断包已保存到所选位置，未上传。"); }
    });
  }
  return <section className="settings-card settings-group-card diagnostic-settings" aria-label="诊断包">
    <div className="settings-group-header">
      <div className="settings-group-title-wrap">
        <span className="settings-group-icon"><FileSearch size={16} aria-hidden="true" /></span>
        <div><h3>诊断包</h3><p className="settings-group-desc">遇到播放或下载问题时，可先预览诊断信息，再保存到本地，方便排查原因。</p></div>
      </div>
      <button type="button" className="btn btn-secondary btn-sm" disabled={busy} onClick={() => void generate()}>{busy && !preview ? "正在生成…" : "预览诊断包"}</button>
    </div>
    <div className="settings-group-body diagnostic-settings-body">
    <p className="settings-field-hint">包含应用版本、系统信息、匿名音频设备、下载状态统计和近期错误类别。点击预览后才会收集，不会自动上传。</p>
    <p className="settings-field-hint">不包含令牌、网址、文件路径、作品标题、设备名称、媒体或数据库；日志只保留时间、级别和错误类别，不导出原始消息。</p>
    {error && !preview && <p className="error-banner" role="alert">{error}</p>}
    {notice && <p className="settings-field-hint" role="status">{notice}</p>}
    </div>
    {preview && <DiagnosticDialog preview={preview} busy={busy} error={error} onClose={() => { if (!busy) { setPreview(null); setError(""); } }} onRefresh={() => void generate()} onExport={() => void exportPreview()} />}
  </section>;
}

function DiagnosticDialog({ preview, busy, error, onClose, onRefresh, onExport }: {
  preview: DiagnosticPreview; busy: boolean; error: string; onClose: () => void; onRefresh: () => void; onExport: () => void;
}) {
  const dialog = useRef<HTMLDialogElement>(null), cancel = useRef<HTMLButtonElement>(null);
  const focused = useRef(false);
  useEffect(() => {
    const el = dialog.current, previous = document.activeElement as HTMLElement | null;
    const main = document.querySelector<HTMLElement>(".main-content"), overflow = main?.style.overflow ?? "";
    el?.showModal(); if (main) main.style.overflow = "hidden";
    cancel.current?.focus();
    return () => { el?.close(); if (main) main.style.overflow = overflow; if (previous?.isConnected) previous.focus({ preventScroll: true }); };
  }, []);
  useEffect(() => { if (!busy && !focused.current) { cancel.current?.focus(); focused.current = true; } }, [busy]);
  return createPortal(<dialog ref={dialog} className="modal-card storage-dialog diagnostic-dialog" aria-modal="true" aria-labelledby="diagnostic-title" aria-describedby="diagnostic-description" aria-busy={busy}
    onCancel={e => { e.preventDefault(); if (!busy) onClose(); }} onKeyDown={e => {
      e.stopPropagation(); if (e.key !== "Tab") return;
      const controls = Array.from(dialog.current?.querySelectorAll<HTMLElement>('button:not(:disabled),[tabindex="0"]') ?? []);
      const first = controls[0], last = controls.at(-1);
      if (e.shiftKey && document.activeElement === first) { e.preventDefault(); last?.focus(); }
      else if (!e.shiftKey && document.activeElement === last) { e.preventDefault(); first?.focus(); }
    }}>
    <header className="storage-heading"><h2 id="diagnostic-title">诊断包预览</h2><button type="button" className="btn-icon" aria-label="关闭诊断预览" title="关闭诊断预览" disabled={busy} onClick={onClose}><X size={18} /></button></header>
    <p id="diagnostic-description" className="settings-field-hint">以下是将保存的完整内容（{formatBytes(preview.bytes)}）。预览有效期为 15 分钟；刷新会重新收集，导出使用当前预览。</p>
    <ul className="diagnostic-scope"><li>版本、系统与匿名音频设备信息</li><li>播放状态与下载任务数量，不含作品或文件信息</li><li>最近最多 100 条警告 / 错误的时间、级别和类别</li></ul>
    <p className="settings-field-hint">生成于 {new Date(preview.generatedAt).toLocaleString()}。设备读取失败、日志读取不完整会在内容中标明。选择新的 JSON 文件名，不覆盖已有文件，不自动上传。</p>
    <pre className="diagnostic-content" tabIndex={0} aria-label="诊断包完整内容">{preview.content}</pre>
    <p className="diagnostic-digest">SHA-256：{preview.sha256}</p>
    {error && <p className="error-banner" role="alert">{error}</p>}
    <footer className="storage-actions"><button ref={cancel} type="button" className="btn btn-ghost" disabled={busy} onClick={onClose}>取消</button>
      <button type="button" className="btn btn-secondary" disabled={busy} onClick={onRefresh}>刷新预览</button>
      <button type="button" className="btn btn-primary" disabled={busy} onClick={onExport}><Download size={16} /><span className="ui-icon-label">{busy ? "正在处理…" : "保存诊断包"}</span></button></footer>
  </dialog>, document.body);
}
