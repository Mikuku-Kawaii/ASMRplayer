import { useCallback, useEffect, useId, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { HardDrive, RefreshCw, Trash2, X } from "lucide-react";
import { callCommand, formatBytes, normalizeAppError } from "../../lib/api";

export type StorageCategory = "playback" | "metadata" | "incomplete" | "offline";
export interface StorageEntry { id: string; category: StorageCategory; path: string; bytes: number; reason: string | null }
export interface StorageOverview { entries: StorageEntry[]; warnings: string[]; databaseMetadataBytes: number }
export interface CleanupPreview { token: string; entries: StorageEntry[]; reclaimableBytes: number }
interface CleanupResult { deletedCount: number; freedBytes: number; skipped: string[] }
const categories: { id: StorageCategory; label: string; hint: string }[] = [
  { id: "playback", label: "播放缓存", hint: "磁盘上的播放临时文件；内存缓冲不计入磁盘用量。" },
  { id: "metadata", label: "封面与元数据缓存", hint: "封面、缩略图、波形和字幕。仍被音频库引用的文件保留。" },
  { id: "incomplete", label: "未完成下载", hint: "已登记的 .part 文件。清理后保留任务，下次从头下载。" },
  { id: "offline", label: "已下载离线文件", hint: "独立选择并删除下载副本，保留作品、进度和书签。" },
];

export function StorageSettings() {
  const [overview, setOverview] = useState<StorageOverview | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [result, setResult] = useState<CleanupResult | null>(null);
  const [preview, setPreview] = useState<CleanupPreview | null>(null);
  const [category, setCategory] = useState<StorageCategory>("playback");
  const [offlineOpen, setOfflineOpen] = useState(false);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [query, setQuery] = useState("");
  const [limit, setLimit] = useState(50);
  const locked = useRef(false);
  const mounted = useRef(true);
  const trigger = useRef<HTMLElement | null>(null);
  useEffect(() => { if (!busy && !preview && trigger.current) { trigger.current.focus({ preventScroll: true }); trigger.current = null; } }, [busy, preview]);
  const refresh = useCallback(async () => {
    const next = await callCommand<StorageOverview>("get_storage_overview");
    if (mounted.current) { setOverview(next); setSelected(new Set()); }
  }, []);
  useEffect(() => {
    mounted.current = true; let current = true;
    setBusy(true); locked.current = true;
    void callCommand<StorageOverview>("get_storage_overview").then(next => { if (current) setOverview(next); })
      .catch(e => { if (current) setError(normalizeAppError(e).message); })
      .finally(() => { if (current) { setBusy(false); locked.current = false; } });
    return () => { current = false; mounted.current = false; };
  }, []);
  async function run(action: () => Promise<void>) {
    if (locked.current) return;
    locked.current = true; setBusy(true); setError("");
    try { await action(); } catch (e) { if (mounted.current) setError(normalizeAppError(e).message); }
    finally { locked.current = false; if (mounted.current) setBusy(false); }
  }
  async function prepare(id: StorageCategory) {
    trigger.current = document.activeElement as HTMLElement;
    setResult(null); setCategory(id);
    await run(async () => {
      const next = await callCommand<CleanupPreview>("preview_storage_cleanup", { category: id, ids: id === "offline" ? [...selected] : [] });
      if (mounted.current) setPreview(next);
    });
  }
  const offline = overview?.entries.filter(e => e.category === "offline") ?? [];
  const filtered = offline.filter(e => e.path.toLowerCase().includes(query.toLowerCase()));
  return <section className="settings-card settings-group-card storage-settings" aria-label="存储用量与清理">
    <div className="storage-heading"><h3><HardDrive size={18} /><span className="ui-icon-label">存储用量与清理</span></h3>
      <button type="button" className="btn btn-ghost btn-sm" disabled={busy} onClick={() => void run(refresh)}><RefreshCw size={14} /><span className="ui-icon-label">{busy && !preview ? "正在读取…" : "刷新用量"}</span></button></div>
    <p className="settings-field-hint">只统计受管理文件的逻辑大小，实际磁盘释放量可能不同。清理需要先预览；导入目录、本地原件与正在使用的文件保留。</p>
    {error && !preview && <p className="error-banner" role="alert">{error}</p>}
    {!overview && <p className="settings-field-hint" role="status">{busy ? "正在统计存储用量…" : "尚未读取存储用量，请刷新重试。"}</p>}
    {overview && <>
      <div className="storage-categories">{categories.map(c => {
        const entries = overview.entries.filter(e => e.category === c.id);
        const bytes = entries.reduce((sum, e) => sum + e.bytes, 0);
        const eligible = entries.filter(e => !e.reason);
        return <div className="storage-category" key={c.id}>
          <h4>{c.label}</h4><strong className="storage-size">{formatBytes(bytes)}</strong>
          <p className="settings-field-hint">{entries.length} 个文件 · 可清理 {formatBytes(eligible.reduce((sum, e) => sum + e.bytes, 0))}</p>
          <p className="settings-field-hint storage-category-hint">{c.hint}</p>
          <button type="button" className="btn btn-secondary btn-sm" disabled={busy || !entries.length} aria-expanded={c.id === "offline" ? offlineOpen : undefined}
            onClick={() => c.id === "offline" ? setOfflineOpen(v => !v) : void prepare(c.id)}>{c.id === "offline" ? "管理离线文件" : `预览清理${c.id === "playback" ? "播放缓存" : c.id === "metadata" ? "元数据缓存" : "未完成下载"}`}</button>
        </div>;
      })}</div>
      <p className="settings-field-hint">数据库内另有 {formatBytes(overview.databaseMetadataBytes)} 的站点元数据与目录树（保留，不纳入文件清理）；数据库、日志及内存缓存不计入上方用量。无法访问或未登记的文件不估算大小。</p>
      {overview.warnings.length > 0 && <details className="storage-details"><summary>有 {overview.warnings.length} 处无法完整统计</summary><ul>{overview.warnings.map((w,i) => <li key={i}>{w}</li>)}</ul></details>}
      {offlineOpen && <div className="storage-offline" role="region" aria-label="管理离线文件">
        <div className="storage-heading"><h4>选择要删除的下载副本</h4><span className="settings-field-hint">已选 {selected.size} 个</span></div>
        <label className="storage-search">查找文件<input className="input-text" value={query} placeholder="输入文件名或路径" onChange={event => { setQuery(event.target.value); setLimit(50); }} /></label>
        <div className="storage-file-list">{filtered.slice(0, limit).map(e => <label className="storage-file-choice" key={e.id}>
          <input type="checkbox" checked={selected.has(e.id)} disabled={busy || !!e.reason} onChange={event => { const next = new Set(selected); if (event.target.checked) next.add(e.id); else next.delete(e.id); setSelected(next); }} />
          <span className="storage-path">{e.path}<small>{formatBytes(e.bytes)}{e.reason ? ` · ${e.reason}` : ""}</small></span>
        </label>)}{!filtered.length && <p className="settings-field-hint">没有匹配的离线文件。</p>}</div>
        <div className="storage-actions">{filtered.length > limit && <button type="button" className="btn btn-ghost btn-sm" onClick={() => setLimit(n => n + 50)}>显示更多文件</button>}
          <button type="button" className="btn btn-secondary btn-sm" disabled={busy || !selected.size} onClick={() => void prepare("offline")}><Trash2 size={14} /><span className="ui-icon-label">预览删除所选副本</span></button></div>
      </div>}
    </>}
    {result && <div className="storage-result"><p role="status">已删除 {result.deletedCount} 个文件，释放约 {formatBytes(result.freedBytes)}；跳过 {result.skipped.length} 个文件。</p>
      {!!result.skipped.length && <details className="storage-details"><summary>查看跳过原因</summary><ul>{result.skipped.map((s,i) => <li key={i}>{s}</li>)}</ul></details>}</div>}
    {preview && <StoragePreviewDialog preview={preview} category={category} busy={busy} error={error} onClose={() => { if (!busy) { setPreview(null); setError(""); } }}
      onConfirm={() => void run(async () => {
        // Tokens are single use, including a partially successful operation. Always require a new preview.
        try {
          const next = await callCommand<CleanupResult>("execute_storage_cleanup", { token: preview.token });
          if (mounted.current) { setResult(next); setPreview(null); }
        } catch (e) { if (mounted.current) setPreview(null); throw e; }
        await refresh();
      })} />}
  </section>;
}

function StoragePreviewDialog({ preview, category, busy, error, onClose, onConfirm }: {
  preview: CleanupPreview; category: StorageCategory; busy: boolean; error: string; onClose: () => void; onConfirm: () => void;
}) {
  const ref = useRef<HTMLDialogElement>(null);
  const cancel = useRef<HTMLButtonElement>(null);
  const initialFocus = useRef(false);
  const title = useId(), description = useId();
  const [limit, setLimit] = useState(50);
  useEffect(() => {
    const previous = document.activeElement as HTMLElement | null;
    const dialog = ref.current;
    dialog?.showModal();
    return () => { dialog?.close(); if (previous?.isConnected) previous.focus({ preventScroll: true }); };
  }, []);
  useEffect(() => { if (!busy && !initialFocus.current) { cancel.current?.focus(); initialFocus.current = true; } }, [busy]);
  const eligible = preview.entries.filter(e => !e.reason);
  return createPortal(<dialog ref={ref} className="modal-card storage-dialog" aria-modal="true" aria-labelledby={title} aria-describedby={description} aria-busy={busy}
    onCancel={event => { event.preventDefault(); if (!busy) onClose(); }} onKeyDown={event => {
      event.stopPropagation();
      if (event.key !== "Tab") return;
      const controls = Array.from(ref.current?.querySelectorAll<HTMLElement>('button:not(:disabled), [tabindex="0"]') ?? []);
      const first = controls[0], last = controls[controls.length - 1];
      if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last?.focus(); }
      else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first?.focus(); }
    }}>
    <header className="storage-heading"><h2 id={title}>{category === "offline" ? "删除离线副本预览" : "清理预览"}</h2><button type="button" className="btn-icon" title="关闭清理预览" aria-label="关闭清理预览" disabled={busy} onClick={onClose}><X size={18} /></button></header>
    <p id={description} className="settings-field-hint">{categories.find(c => c.id === category)?.label} · 预计删除 {eligible.length} 个文件，释放约 <strong>{formatBytes(preview.reclaimableBytes)}</strong>。跳过 {preview.entries.length - eligible.length} 个受保护文件。预览 5 分钟内有效，确认时再次检查。</p>
    <p className="settings-field-hint">{category === "offline" ? "删除后需要重新下载才能离线播放。作品索引、进度、书签和本地导入原件保留。" : category === "incomplete" ? "未完成的下载内容会丢弃；任务保留，之后可重新下载。" : "仅删除下列可清理的缓存文件，离线音频和本地导入原件保留。"}</p>
    <div className="storage-file-list" tabIndex={0} aria-label="清理文件范围">{preview.entries.slice(0,limit).map(e => <div className="storage-preview-file" key={e.id}><span className="storage-path">{e.path}<small>{e.reason ? `保留 · ${e.reason}` : "将删除"}</small></span><span className="storage-file-size">{formatBytes(e.bytes)}</span></div>)}
      {!preview.entries.length && <p className="settings-field-hint">没有可清理的文件。</p>}
      {preview.entries.length > limit && <button type="button" className="btn btn-ghost btn-sm" onClick={() => setLimit(n => n + 50)}>显示更多文件（剩余 {preview.entries.length - limit} 个）</button>}</div>
    {error && <p className="error-banner" role="alert">{error}</p>}
    <footer className="storage-actions"><button ref={cancel} type="button" className="btn btn-ghost" disabled={busy} onClick={onClose}>取消</button>
      <button type="button" className={`btn ${category === "offline" ? "btn-danger" : "btn-primary"}`} disabled={busy || !eligible.length} onClick={onConfirm}>{busy ? "正在清理…" : category === "offline" ? "删除所选离线副本" : "确认清理"}</button></footer>
  </dialog>, document.body);
}
