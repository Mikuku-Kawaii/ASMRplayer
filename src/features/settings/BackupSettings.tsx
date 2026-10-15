import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { save } from "@tauri-apps/plugin-dialog";
import { ArchiveRestore, Download, RefreshCw, X } from "lucide-react";
import { Select } from "../../components/Select";
import { callCommand, formatBytes, normalizeAppError, type BackupRestoreReportDto } from "../../lib/api";

export interface BackupPolicy { intervalHours: number; keepCount: number }
export interface BackupFile { id: string; createdAt: string; kind: string; bytes: number; sha256: string }
export interface BackupStatus { policy: BackupPolicy; files: BackupFile[]; lastSuccess: string | null; lastError: string | null; nextDue: string | null; directory: string }
export type RestoreScope = "favorites" | "positions" | "bookmarks" | "abLoops" | "metadata" | "presets" | "workDsp" | "globalDsp";
export interface RestorePreview { token: string | null; schemaVersion: number; exportedAt: string; compatible: boolean; issue: string | null; counts: { scope: RestoreScope; count: number }[]; matched: BackupRestoreReportDto | null; safetyBackup: BackupFile | null }
interface RestoreResult { report: BackupRestoreReportDto; safetyBackup: BackupFile; dspChanged: boolean; runtimeWarning?: string | null }
const labels: Record<RestoreScope,string> = { favorites: "喜欢", positions: "收听进度", bookmarks: "书签笔记", abLoops: "A–B 循环", metadata: "元数据覆盖", presets: "自定义音效预设", workDsp: "作品音效", globalDsp: "全局音效" };
const date = (value: string | null) => { if (!value) return "尚无记录"; const d = new Date(value); return Number.isNaN(d.getTime()) ? "时间无效" : d.toLocaleString(); };
const kind = (value: string) => value === "automatic" ? "自动备份" : value === "before_restore" ? "恢复前副本" : "手动备份";

export function BackupSettings({ onRestored }: { onRestored: () => Promise<void> }) {
  const [status, setStatus] = useState<BackupStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [preview, setPreview] = useState<RestorePreview | null>(null);
  const [scopes, setScopes] = useState<RestoreScope[]>([]);
  const [passphrase, setPassphrase] = useState("");
  const [limit, setLimit] = useState(10);
  const input = useRef<HTMLInputElement>(null), encryptedInput = useRef<HTMLInputElement>(null);
  const locked = useRef(false), mounted = useRef(true);
  const statusRequest = useRef(0);
  const trigger = useRef<HTMLElement | null>(null);
  useEffect(() => { if (!busy && !preview && trigger.current) { trigger.current.focus({ preventScroll: true }); trigger.current = null; } }, [busy,preview]);
  async function reload() { const request = ++statusRequest.current; const next = await callCommand<BackupStatus>("get_backup_status"); if (mounted.current && request === statusRequest.current) setStatus(next); }
  useEffect(() => {
    mounted.current = true; let active = true;
    const refresh = async () => {
      if (locked.current) return;
      const request = ++statusRequest.current;
      try { const next = await callCommand<BackupStatus>("get_backup_status"); if (active && request === statusRequest.current) setStatus(next); }
      catch (e) { if (active && request === statusRequest.current) setError(normalizeAppError(e).message); }
    };
    void refresh(); const timer = window.setInterval(() => void refresh(),60_000);
    return () => { active = false; mounted.current = false; window.clearInterval(timer); };
  }, []);
  async function run(action: () => Promise<void>) {
    if (locked.current) return;
    locked.current = true; ++statusRequest.current; setBusy(true); setError(""); setNotice("");
    try { await action(); } catch (e) { if (mounted.current) { const err=normalizeAppError(e); setError([err.message,err.hint].filter(Boolean).join(" ")); } }
    finally { locked.current = false; if (mounted.current) setBusy(false); }
  }
  async function prepare(raw: string | null, fileId: string | null, encrypted: boolean) {
    trigger.current = document.activeElement as HTMLElement;
    const next = await callCommand<RestorePreview>("preview_user_backup",{raw,fileId,passphrase: encrypted ? passphrase : null});
    if (mounted.current) { setPreview(next); setScopes(next.counts.filter(c => c.count>0).map(c=>c.scope)); if (encrypted) setPassphrase(""); }
    await reload();
  }
  async function readFile(file: File | undefined, encrypted: boolean) {
    if (!file) return;
    await run(async () => { if (file.size > 64*1024*1024) throw new Error("备份文件超过 64 MB"); await prepare(await file.text(),null,encrypted); });
  }
  async function exportFile(encrypted: boolean) {
    await run(async () => {
      const target = await save({ title: encrypted ? "导出加密同步包" : "导出用户资产备份", defaultPath: `asmr-player-${encrypted ? "encrypted" : "backup"}-${new Date().toISOString().slice(0,10)}.json`, filters:[{ name:"JSON",extensions:["json"] }] });
      if (!target) return;
      await callCommand(encrypted ? "export_encrypted_sync_bundle" : "export_user_backup",{targetPath:target,...(encrypted ? {passphrase} : {})});
      if (mounted.current) setNotice("备份已导出到所选文件。");
    });
  }
  const changePolicy = (policy: BackupPolicy) => void run(async () => {
    const next = await callCommand<BackupStatus>("set_backup_policy",{policy});
    if (mounted.current) { setStatus(next); setNotice("备份策略已保存。保留策略在下一次自动备份成功后执行。"); }
  });
  return <section className="settings-card settings-group-card backup-settings" data-settings-group="backup-sync" aria-label="用户资产备份与多端加密同步">
    <div className="storage-heading"><h3><ArchiveRestore size={18} /><span className="ui-icon-label">自动备份与恢复</span></h3><button type="button" className="btn btn-ghost btn-sm" disabled={busy} onClick={() => void run(reload)}><RefreshCw size={14} /><span className="ui-icon-label">刷新备份</span></button></div>
    <p className="settings-field-hint">备份喜欢、进度、书签、A–B 循环、元数据覆盖与音效配置。媒体文件、音频库索引、播放队列方案、快捷键及应用设置不包含在此格式中。</p>
    {status ? <>
      <div className="backup-policy">
        <label>自动备份周期<Select aria-label="自动备份周期" value={String(status.policy.intervalHours)} disabled={busy} options={[{value:"0",label:"关闭"},{value:"24",label:"每天"},{value:"72",label:"每 3 天"},{value:"168",label:"每周"}]} onValueChange={value=>changePolicy({...status.policy,intervalHours:Number(value)})} /></label>
        <label>保留自动副本<Select aria-label="保留自动副本" value={String(status.policy.keepCount)} disabled={busy} options={Array.from({length:30},(_,i)=>({value:String(i+1),label:`${i+1} 份`}))} onValueChange={value=>changePolicy({...status.policy,keepCount:Number(value)})} /></label>
      </div>
      <p className="settings-field-hint">仅在应用运行时自动备份（含托盘）；错过周期后补做一次。只有新副本写入并校验成功后才清理超额自动副本。手动与恢复前副本一直保留。</p>
      <dl className="backup-status"><div><dt>最近自动备份成功</dt><dd>{date(status.lastSuccess)}</dd></div><div><dt>下次计划</dt><dd>{status.policy.intervalHours ? status.nextDue ? date(status.nextDue) : "等待应用内定时检查" : "自动备份已关闭"}</dd></div><div><dt>保存目录</dt><dd>{status.directory}</dd></div></dl>
      {status.lastError && <p className="error-banner" role="alert">{status.lastError}</p>}
    </> : <p className="settings-field-hint">尚未读取备份策略，可刷新重试。</p>}
    <div className="backup-toolbar"><button type="button" className="btn btn-primary btn-sm" disabled={busy || !status} onClick={()=>void run(async()=>{ await callCommand("create_user_backup"); await reload(); if(mounted.current)setNotice("手动备份已保存并校验。"); })}>{busy ? "正在处理…" : "立即备份"}</button>
      <button type="button" className="btn btn-secondary btn-sm" disabled={busy} onClick={()=>void exportFile(false)}><Download size={14} /><span className="ui-icon-label">导出备份 JSON</span></button>
      <button type="button" className="btn btn-secondary btn-sm" disabled={busy} onClick={()=>input.current?.click()}>从备份文件恢复</button>
      <input ref={input} hidden type="file" accept=".json,application/json" data-testid="backup-restore-input" onChange={e=>{ const file=e.target.files?.[0];e.target.value="";void readFile(file,false); }} />
    </div>
    <div className="backup-encrypted"><label>加密同步口令<input type="password" autoComplete="off" className="input-text" placeholder="输入口令以导出或预览加密同步包" value={passphrase} disabled={busy} onChange={e=>setPassphrase(e.target.value)} /></label>
      <div className="backup-toolbar"><button type="button" className="btn btn-secondary btn-sm" disabled={busy || passphrase.trim().length<4} onClick={()=>void exportFile(true)}>导出加密同步包</button>
        <button type="button" className="btn btn-secondary btn-sm" disabled={busy || !passphrase.trim()} onClick={()=>encryptedInput.current?.click()}>导入加密同步包</button>
        <input ref={encryptedInput} hidden type="file" accept=".json,application/json" data-testid="encrypted-sync-input" onChange={e=>{const file=e.target.files?.[0];e.target.value="";void readFile(file,true);}} /></div></div>
    {status && <div className="backup-files"><h4>本机副本 ({status.files.length})</h4>
      {!status.files.length && <p className="settings-field-hint">尚无备份。点击“立即备份”创建第一份副本，或开启自动备份。</p>}
      {status.files.slice(0,limit).map(file=><div className="backup-file" key={file.id}><span>{kind(file.kind)} · {date(file.createdAt)}<small>{formatBytes(file.bytes)} · {file.id}</small></span><button type="button" className="btn btn-secondary btn-sm" disabled={busy} aria-label={`预览${kind(file.kind)} ${date(file.createdAt)}`} onClick={()=>void run(()=>prepare(null,file.id,false))}>预览恢复</button></div>)}
      {status.files.length>limit && <button type="button" className="btn btn-ghost btn-sm" onClick={()=>setLimit(n=>n+10)}>显示更多副本</button>}</div>}
    {error && !preview && <p className="error-banner" role="alert">{error}</p>}{notice && <p className="settings-field-hint" role="status">{notice}</p>}
    {preview && <RestoreDialog preview={preview} scopes={scopes} busy={busy} error={error} onScopes={setScopes} onClose={()=>{if(!busy){setPreview(null);setError("");}}} onConfirm={()=>void run(async()=>{
      let result: RestoreResult;
      try { result=await callCommand<RestoreResult>("apply_user_backup",{token:preview.token,scopes}); }
      catch(e){ if(mounted.current)setPreview(null);throw e; }
      if(mounted.current){setPreview(null);setNotice(`恢复完成：喜欢 ${result.report.favoritesRestored} 项 · 进度 ${result.report.positionsRestored} 项 · 书签 ${result.report.bookmarksRestored} 项。执行前副本已保留。${result.runtimeWarning ?? ""}`);}
      await reload(); await onRestored();
    })} />}
  </section>;
}

function RestoreDialog({preview,scopes,busy,error,onScopes,onClose,onConfirm}:{preview:RestorePreview;scopes:RestoreScope[];busy:boolean;error:string;onScopes:(v:RestoreScope[])=>void;onClose:()=>void;onConfirm:()=>void}) {
  const dialog=useRef<HTMLDialogElement>(null),cancel=useRef<HTMLButtonElement>(null);const focused=useRef(false);
  useEffect(()=>{
    const el=dialog.current;const previous=document.activeElement as HTMLElement|null;const main=document.querySelector<HTMLElement>(".main-content");const overflow=main?.style.overflow??"";
    el?.showModal();if(main)main.style.overflow="hidden";
    return ()=>{el?.close();if(main)main.style.overflow=overflow;if(previous?.isConnected)previous.focus({preventScroll:true});};
  },[]);
  useEffect(()=>{if(!busy&&!focused.current){cancel.current?.focus();focused.current=true;}},[busy]);
  return createPortal(<dialog ref={dialog} className="modal-card storage-dialog backup-dialog" aria-modal="true" aria-labelledby="backup-preview-title" aria-describedby="backup-preview-description" aria-busy={busy} onCancel={e=>{e.preventDefault();if(!busy)onClose();}} onKeyDown={e=>{
    e.stopPropagation();if(e.key!=="Tab")return;const controls=Array.from(dialog.current?.querySelectorAll<HTMLElement>('button:not(:disabled),[tabindex="0"]')??[]);const first=controls[0],last=controls.at(-1);
    if(e.shiftKey&&document.activeElement===first){e.preventDefault();last?.focus();}else if(!e.shiftKey&&document.activeElement===last){e.preventDefault();first?.focus();}
  }}>
    <header className="storage-heading"><h2 id="backup-preview-title">恢复备份预览</h2><button type="button" className="btn-icon" aria-label="关闭恢复预览" title="关闭恢复预览" disabled={busy} onClick={onClose}><X size={18}/></button></header>
    <p id="backup-preview-description" className="settings-field-hint">格式版本 {preview.schemaVersion} · 备份时间 {date(preview.exportedAt)} · {preview.compatible ? "兼容当前版本" : "无法恢复"}</p>
    {preview.issue && <p className="error-banner" role="alert">{preview.issue}</p>}
    {preview.safetyBackup && <p className="backup-safety">已保存当前用户资产副本：{date(preview.safetyBackup.createdAt)}。确认前会再次备份最新数据；任一次备份失败均不恢复。</p>}
    {!!preview.counts.length && <div className="backup-scopes" role="group" aria-label="选择恢复范围">{preview.counts.map(c=><button type="button" key={c.scope} className={`backup-scope ${scopes.includes(c.scope)?"is-selected":""}`} aria-pressed={scopes.includes(c.scope)} disabled={busy || !preview.compatible || !c.count} onClick={()=>onScopes(scopes.includes(c.scope)?scopes.filter(s=>s!==c.scope):[...scopes,c.scope])}><span>{labels[c.scope]}</span><span>{c.count} 项</span></button>)}</div>}
    <p className="settings-field-hint">恢复采用合并：喜欢只添加，较新进度和已有书签保留；所选元数据、循环及音效可覆盖现值。进度与循环仅匹配相同音轨版本 ID；缺失版本的书签保留为未关联笔记。不会重建音频库索引或改动媒体文件。</p>
    {preview.matched && <p className="settings-field-hint">全部范围试检可匹配：喜欢 {preview.matched.favoritesRestored} 项、进度 {preview.matched.positionsRestored} 项、循环 {preview.matched.abLoopsRestored} 项；书签可新增 {preview.matched.bookmarksRestored} 项。未匹配或已有记录可能跳过，最终按所选范围重新检查。</p>}
    {error && <p className="error-banner" role="alert">{error}</p>}
    <footer className="storage-actions"><button ref={cancel} type="button" className="btn btn-ghost" disabled={busy} onClick={onClose}>取消</button><button type="button" className="btn btn-primary" disabled={busy || !preview.compatible || !preview.token || !scopes.length} onClick={onConfirm}>{busy?"正在备份并恢复…":"备份并恢复所选范围"}</button></footer>
  </dialog>,document.body);
}
