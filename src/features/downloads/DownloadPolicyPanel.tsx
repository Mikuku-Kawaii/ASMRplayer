import { useEffect, useRef, useState } from "react";
import { Clock3, Settings2 } from "lucide-react";
import { Select } from "../../components/Select";
import { callCommand, normalizeAppError } from "../../lib/api";

export interface DownloadPolicy {
  concurrency: number;
  speedLimitKib: number;
  scheduleEnabled: boolean;
  startMinute: number;
  endMinute: number;
  autoResumeNetwork: boolean;
  resumeOnStartup: boolean;
}
export interface DownloadPolicyStatus { policy: DownloadPolicy; scheduleOpen: boolean; activeTasks: number }
const defaults: DownloadPolicy = {concurrency:2,speedLimitKib:0,scheduleEnabled:false,startMinute:0,endMinute:420,autoResumeNetwork:false,resumeOnStartup:false};
const time = (minute: number) => `${Math.floor(minute/60).toString().padStart(2,"0")}:${(minute%60).toString().padStart(2,"0")}`;
const minute = (value: string) => { const [h,m]=value.split(":").map(Number); return value ? h!*60+m! : -1; };
export function DownloadPolicyPanel() {
  const [status,setStatus]=useState<DownloadPolicyStatus|null>(null);
  const [draft,setDraft]=useState(defaults);
  const [busy,setBusy]=useState(false);
  const [error,setError]=useState("");
  const [saved,setSaved]=useState(false);
  const mounted=useRef(false),locked=useRef(false),dirty=useRef(false),request=useRef(0);
  useEffect(()=>{
    mounted.current=true;
    const refresh=async()=>{
      if(locked.current)return;
      const id=++request.current;
      try {
        const next=await callCommand<DownloadPolicyStatus>("get_download_policy");
        if(mounted.current&&id===request.current&&next){setStatus(next);if(!dirty.current)setDraft(next.policy);}
      } catch(e){if(mounted.current&&id===request.current)setError(normalizeAppError(e).message);}
    };
    void refresh();const timer=window.setInterval(()=>void refresh(),5000);
    return()=>{mounted.current=false;window.clearInterval(timer);};
  },[]);
  function edit(patch: Partial<DownloadPolicy>){dirty.current=true;setSaved(false);setDraft(p=>({...p,...patch}));}
  async function save(){
    if(locked.current)return;
    if(!Number.isInteger(draft.speedLimitKib)||draft.speedLimitKib<0||draft.speedLimitKib>102400||(draft.speedLimitKib>0&&draft.speedLimitKib<16)) {setError("总限速请输入 0 或 16–102400 KiB/s。");return;}
    if(draft.startMinute<0||draft.endMinute<0||(draft.scheduleEnabled&&draft.startMinute===draft.endMinute)){setError("请选择不同的开始与结束时间。");return;}
    locked.current=true;++request.current;setBusy(true);setError("");setSaved(false);
    try {
      const next=await callCommand<DownloadPolicyStatus>("set_download_policy",{policy:draft});
      if(mounted.current){setStatus(next);setDraft(next.policy);dirty.current=false;setSaved(true);}
    } catch(e){if(mounted.current)setError(normalizeAppError(e).message);}
    finally{locked.current=false;if(mounted.current)setBusy(false);}
  }
  const current=status?.policy;
  return <section className="settings-card download-policy" aria-label="下载调度与网络偏好">
    <div className="download-policy-heading"><h3><Settings2 size={17}/>下载调度与网络偏好</h3><button type="button" className="btn btn-primary btn-sm" disabled={busy} onClick={()=>void save()}>{busy?"正在保存…":"保存策略"}</button></div>
    <p className="download-policy-effective" role="status">{current ? `当前生效：并发 ${current.concurrency} · ${current.speedLimitKib ? `总限速 ${current.speedLimitKib} KiB/s` : "不限速"} · ${current.scheduleEnabled ? `${time(current.startMinute)}–${time(current.endMinute)}${current.startMinute>current.endMinute ? "（跨午夜）" : ""}，${status.scheduleOpen ? "下载时段内" : "等待下载时段"}` : "全天下载"} · 网络中断后${current.autoResumeNetwork?"自动重试":"手动继续"} · ${current.resumeOnStartup?"重启后继续":"重启后暂停未完成任务"} · 正在处理 ${status.activeTasks} 项` : "尚未读取生效策略，可重新保存策略。"}</p>
    <div className="download-policy-grid">
      <label>最大并发<Select aria-label="最大并发" value={String(draft.concurrency)} disabled={busy} options={Array.from({length:6},(_,i)=>({value:String(i+1),label:`${i+1} 个任务`}))} onValueChange={v=>edit({concurrency:Number(v)})}/></label>
      <label>总限速 (KiB/s)<input className="input-text" type="number" min={0} max={102400} step={1} value={draft.speedLimitKib} disabled={busy} onChange={e=>edit({speedLimitKib:Number(e.target.value)})}/></label>
      <label>下载时段<Select aria-label="下载时段" value={draft.scheduleEnabled?"scheduled":"all"} disabled={busy} options={[{value:"all",label:"全天下载"},{value:"scheduled",label:"每天指定时段"}]} onValueChange={v=>edit({scheduleEnabled:v==="scheduled"})}/></label>
      {draft.scheduleEnabled&&<><label>开始时间<input className="input-text" type="time" value={draft.startMinute>=0?time(draft.startMinute):""} disabled={busy} onChange={e=>edit({startMinute:minute(e.target.value)})}/></label><label>结束时间<input className="input-text" type="time" value={draft.endMinute>=0?time(draft.endMinute):""} disabled={busy} onChange={e=>edit({endMinute:minute(e.target.value)})}/></label></>}
    </div>
    <div className="download-policy-toggles">
      <button type="button" className={`btn-chip ${draft.autoResumeNetwork?"active":""}`} aria-pressed={draft.autoResumeNetwork} disabled={busy} onClick={()=>edit({autoResumeNetwork:!draft.autoResumeNetwork})}>网络中断后自动重试</button>
      <button type="button" className={`btn-chip ${draft.resumeOnStartup?"active":""}`} aria-pressed={draft.resumeOnStartup} disabled={busy} onClick={()=>edit({resumeOnStartup:!draft.resumeOnStartup})}>重启后继续未完成下载</button>
    </div>
    <p className="settings-field-hint"><Clock3 size={13}/>时间使用系统本地时区，应用退出后不下载；时段结束保留断点，下次时段继续。限速 0 表示不限速，按所有任务合计；降低并发后，已运行任务完成才补充新任务。</p>
    <p className="settings-field-hint">自动重试只处理网络中断及服务暂时不可用，间隔 30 秒至 5 分钟；手动暂停或取消的任务不会自动继续。优先级影响排队顺序，不打断正在下载的任务。</p>
    {error&&<p role="alert" className="error-banner">{error}</p>}{saved&&<p className="settings-field-hint">策略已保存并生效。</p>}
  </section>;
}
