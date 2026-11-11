import { useEffect, useRef, useState, type ReactNode } from "react";
import { LoaderCircle } from "lucide-react";
import { callCommand, formatBytes, formatDurationMs, normalizeAppError, type DownloadTaskDto } from "../../lib/api";
import type { DeviceSyncStatus, SyncResume } from "../../lib/deviceSync";
import type { SubtitleTaskDto } from "../../lib/recognition";
import { SubtitleTaskPanel } from "../player/SubtitleTaskPanel";
import { ErrorNotice } from "../../components/ErrorNotice";
import { SyncResultDetails } from "../settings/SyncResultDetails";

type ScanTask = { state: string; title?: string; phase?: string; error?: string };
type Row = { id: string; title: string; kind: string; state: string; detail: string; error?: string | null; progress?: number;
  actions: { label: string; run: () => void }[] };
const activeStates = new Set(["queued", "resolving", "downloading", "verifying", "running", "cancelling"]);
const stateNames: Record<string,string> = { queued:"等待中",resolving:"解析中",downloading:"下载中",verifying:"校验中",running:"进行中",cancelling:"正在停止",paused:"已暂停",failed:"失败",completed:"已完成",cancelled:"已取消",canceled:"已取消" };
export function ResumeCard({ value, busy, resume, dismiss }: { value: SyncResume; busy: boolean; resume: () => void; dismiss: () => void }) {
  return <section className="task-resume-card" aria-label="跨设备继续收听"><div><strong>继续另一台设备的收听</strong>
    <p>{value.title}</p><small>{value.deviceName} · {formatDurationMs(value.positionMs)} · {new Date(value.updatedMs).toLocaleString()}</small></div>
    <div className="task-actions"><button className="btn btn-primary btn-sm" disabled={busy} onClick={resume}>继续收听</button>
      <button className="btn btn-ghost btn-sm" disabled={busy} onClick={dismiss}>忽略此次</button></div></section>;
}
export function TaskCenter({ sync, syncError, syncBusy, resume, dismissResume, runSync, openSettings, openDownloads, section = "all", onSectionChange, downloadContent }: {
  sync?: DeviceSyncStatus; syncError: string; syncBusy: boolean; resume: () => void; dismissResume: () => void; runSync: () => void;
  openSettings: (section: string) => void; openDownloads: () => void; section?: "all" | "downloads"; onSectionChange?: (section: "all" | "downloads") => void; downloadContent?: ReactNode;
}) {
  const [downloads, setDownloads] = useState<DownloadTaskDto[]>([]), [subtitle, setSubtitle] = useState<SubtitleTaskDto>();
  const [scan, setScan] = useState<ScanTask>(), [errors, setErrors] = useState<string[]>([]);
  const [filter, setFilter] = useState("all"), [busy, setBusy] = useState<string[]>([]), [review, setReview] = useState(false);
  const [revision, setRevision] = useState(0), [actionError, setActionError] = useState("");
  const pending = useRef(new Set<string>());
  const [ready, setReady] = useState(false);
  useEffect(() => {
    if (section === "downloads") return;
    let disposed = false, polling = false;
    const poll = async () => {
      if (polling) return; polling = true;
      const results = await Promise.allSettled([callCommand<DownloadTaskDto[]>("list_download_tasks"),callCommand<SubtitleTaskDto>("get_subtitle_task"),callCommand<ScanTask>("get_scan_task")]);
      if (!disposed) {
        setReady(true);
        if(results[0].status==="fulfilled") setDownloads(results[0].value);
        if(results[1].status==="fulfilled") setSubtitle(results[1].value);
        if(results[2].status==="fulfilled") setScan(results[2].value);
        setErrors(results.flatMap((r,i)=>r.status==="rejected" ? [`${["下载","字幕","扫描"][i]}状态读取失败：${normalizeAppError(r.reason).message}`] : []));
      }
      polling = false;
    };
    void poll(); const timer = window.setInterval(() => void poll(), 2000);
    return () => { disposed = true; window.clearInterval(timer); };
  }, [revision, section]);
  async function act(id: string, command: string, args?: Record<string,unknown>) {
    if(pending.current.has(id)) return;
    pending.current.add(id);setBusy([...pending.current]);setActionError("");
    try { await callCommand(command,args); setRevision(n=>n+1); }
    catch(e) { setActionError(normalizeAppError(e).message); }
    finally { pending.current.delete(id);setBusy([...pending.current]); }
  }
  const rows: Row[] = downloads.map(d => ({id:d.id,title:d.trackTitle || d.workTitle,kind:`下载 · ${d.workTitle}`,state:d.status,
    detail:`${formatBytes(d.downloadedBytes)} / ${d.totalBytes ? formatBytes(d.totalBytes) : "大小未知"}`,error:d.errorMessage,
    progress:d.totalBytes ? Math.min(1,d.downloadedBytes/d.totalBytes) : undefined,
    actions: activeStates.has(d.status) ? [{label:"暂停",run:()=>void act(d.id,"pause_download_task",{taskId:d.id})}] :
      ["paused","failed"].includes(d.status) ? [{label:d.status==="failed"?"重试":"继续",run:()=>void act(d.id,"resume_download_task",{taskId:d.id})}] : []}));
  if(subtitle?.state) rows.unshift({id:"subtitle",title:subtitle.trackTitle || "识别与翻译",kind:subtitle.workTitle ? `字幕 · ${subtitle.workTitle}` : "字幕任务",state:subtitle.state,detail:`${subtitle.phase}${subtitle.total ? ` · ${subtitle.completed} / ${subtitle.total}` : ""}`,error:subtitle.error,
    progress:subtitle.total ? subtitle.completed/subtitle.total : undefined,
    actions:[...(activeStates.has(subtitle.state) ? [{label:"停止",run:()=>void act("subtitle","cancel_subtitle_task")}] : []),{label:activeStates.has(subtitle.state)?"详情":"查看 / 继续处理",run:()=>setReview(v=>!v)}]});
  if(scan && scan.state!=="idle") rows.unshift({id:"scan",title:scan.title || "音频库扫描",kind:"目录扫描",state:scan.state,detail:scan.phase || "",error:scan.error,
    actions:scan.state==="running" ? [{label:"停止",run:()=>void act("scan","cancel_scan")}] : [{label:"管理目录",run:()=>openSettings("storage-library")}]});
  if(sync?.connected) rows.unshift({id:"sync",title:"跨设备同步",kind:"云端同步",state:sync.phase || syncBusy ? "running" : sync.autoError ? "failed" : sync.lastSync ? "completed" : "paused",
    detail:sync.phase || (sync.lastSync ? `上次同步 ${new Date(sync.lastSync).toLocaleString()} · ${sync.pending} 条待匹配` : "等待首次同步"),error:sync.autoError,
    actions:[...(!sync.phase && !syncBusy ? [{label:sync.autoError?"重试":"立即同步",run:runSync}] : []),{label:"绑定与明细",run:()=>openSettings("device-sync")}]});
  const visible = rows.filter(row=>filter==="active" ? activeStates.has(row.state) : filter==="attention" ? ["failed","paused","cancelled","canceled"].includes(row.state) : filter==="done" ? row.state==="completed" : true);
  return <section className="task-center"><header className="task-center-header"><div><h2>任务中心</h2><p>{section === "downloads" ? "管理下载任务与离线文件" : `${rows.filter(r=>activeStates.has(r.state)).length} 项进行中 · ${rows.filter(r=>r.state==="failed").length} 项失败`}</p></div></header>
    <div className="task-sections" role="group" aria-label="任务分类" data-downloads={section === "downloads"}>
      {([ ["all","全部任务"], ["downloads","离线下载"] ] as const).map(([id,label])=><button key={id} className="btn btn-ghost" aria-pressed={section === id} onClick={()=>onSectionChange?.(id)}>{label}</button>)}
    </div>
    {section === "downloads" ? downloadContent : <>
    {sync?.resume && <ResumeCard value={sync.resume} busy={syncBusy} resume={resume} dismiss={dismissResume}/>}
    <div className="task-filters" role="group" aria-label="任务状态">{([["all","全部"],["active","进行中"],["attention","待处理"],["done","已完成"]] as const).map(([id,label])=>
      <button className={`btn-chip ${filter===id?"active":""}`} aria-pressed={filter===id} key={id} onClick={()=>setFilter(id)}>{label}</button>)}</div>
    {[...errors,syncError,actionError].filter(Boolean).map((e,i)=><ErrorNotice key={i} message={e}/>)}
    {errors.length>0 && <button className="btn btn-ghost btn-sm" onClick={()=>setRevision(n=>n+1)}>重新读取</button>}
    <div className="task-list">{visible.slice(0,100).map(row=><article className="task-row" key={row.id}>
      <div className="task-row-content"><small>{row.kind}</small><strong>{row.title}</strong><p>{stateNames[row.state] || row.state} · {row.detail}</p>
        {activeStates.has(row.state) && (row.progress===undefined
          ? <span className="task-loading" role="status" aria-label={`${row.title}处理中`}><LoaderCircle size={16}/></span>
          : <progress aria-label={`${row.title}进度`} max={1} value={row.progress}/>)}
        {row.error && <p className="task-error">{row.error}</p>}</div>
      <div className="task-actions">{row.actions.map(a=><button className="btn btn-secondary btn-sm" disabled={busy.includes(row.id)} key={a.label} onClick={a.run}>{a.label}</button>)}</div>
    </article>)}</div>
    {!ready && <p className="task-loading" role="status"><LoaderCircle size={18}/>正在读取任务…</p>}
    {ready && visible.length===0 && errors.length===0 && <p className="empty-box">此分类暂无任务</p>}
    {visible.length>100 && <button className="btn btn-ghost btn-sm" onClick={openDownloads}>查看其余下载任务</button>}
    {review && subtitle?.versionId && <SubtitleTaskPanel versionId={subtitle.versionId} trackTitle={subtitle.trackTitle || "任务对应音轨"} onOpenSettings={()=>openSettings("recognition-translation")}/>}
    {sync?.lastSync && <details className="task-sync-details"><summary>最近同步明细</summary><SyncResultDetails status={sync}/></details>}
    </>}
  </section>;
}
