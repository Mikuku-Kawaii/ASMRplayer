import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { ArrowLeftRight, Blend, Link2, RefreshCw } from "lucide-react";
import { Select } from "../../components/Select";
import { ErrorNotice } from "../../components/ErrorNotice";
import { callCommand, normalizeAppError } from "../../lib/api";

export interface ImmersionSettings { reverseStereo:boolean; gaplessPlayback:boolean; fadeEnabled:boolean; fadeDurationMs:number }
export function ImmersionPanel() {
  const [settings,setSettings]=useState<ImmersionSettings|null>(null),[busy,setBusy]=useState(false),[error,setError]=useState("");
  const epoch=useRef(0),pending=useRef(false);
  const durationRef = useRef<HTMLDivElement>(null);
  const showDuration = settings?.fadeEnabled ?? false;
  useLayoutEffect(() => {
    if (durationRef.current) durationRef.current.inert = !showDuration;
  }, [settings, showDuration]);
  async function read() {
    const id=++epoch.current;setError("");
    try {const value=await callCommand<ImmersionSettings>("immersion_command");if(id===epoch.current)setSettings(value);}
    catch(e){if(id===epoch.current)setError(normalizeAppError(e).message);}
  }
  useEffect(()=>{void read();const request=epoch;return()=>{request.current++;};},[]);
  async function change(patch:Partial<ImmersionSettings>) {
    if(pending.current||!settings)return;
    pending.current=true;setBusy(true);setError("");const id=++epoch.current;
    try {const value=await callCommand<ImmersionSettings>("immersion_command",{settings:{...settings,...patch}});if(id===epoch.current)setSettings(value);}
    catch(e){if(id===epoch.current)setError(normalizeAppError(e).message);}
    finally{pending.current=false;if(id===epoch.current)setBusy(false);}
  }
  const toggle=(key:"reverseStereo"|"gaplessPlayback"|"fadeEnabled",label:string,english:string,description:string,Icon:typeof ArrowLeftRight)=><button type="button" className="audio-output-mode immersion-setting" role="switch" aria-label={label} aria-checked={settings?.[key]??false} aria-disabled={busy} disabled={!settings} onClick={()=>void change({[key]:!settings?.[key]})}>
    <span className="audio-output-mode-heading"><span className="audio-output-mode-title"><Icon size={18} aria-hidden="true"/><span className="ui-icon-label">{label}</span></span><span className="audio-output-mode-state">{settings?.[key]?"已开启":"已关闭"}</span></span>
    <span className="audio-output-mode-name">{english}</span>
    <span className="audio-output-mode-description">{description}</span>
  </button>;
  return <div className="drawer-body immersion-panel" aria-busy={busy}>
    <p className="text-muted immersion-intro">为侧卧和长时间收听调整播放方式，修改后自动保存。</p>
    {!settings&&!error&&<p role="status">正在读取沉浸设置…</p>}
    {toggle("reverseStereo","声道翻转","Reverse Stereo","将当前输出的左、右声道互换，音效直通时也生效。",ArrowLeftRight)}
    {toggle("gaplessPlayback","无缝播放","Gapless Playback","相同音频参数优先连续衔接；网络缓冲或参数变化仍可能短暂停顿。",Link2)}
    <div>
      {toggle("fadeEnabled","淡入淡出","Fade Out / Fade In","切歌先淡出旧曲，再淡入新曲；暂停和恢复也平滑过渡，不叠加台词。",Blend)}
      <div ref={durationRef} className={`immersion-duration-reveal${showDuration ? " is-open" : ""}`} aria-hidden={!showDuration}>
        <div className="immersion-duration-clip">
          {settings && <div className="immersion-duration">
            <label id="immersion-fade-duration-label" htmlFor="immersion-fade-duration">每段过渡时长</label>
            <Select id="immersion-fade-duration" aria-labelledby="immersion-fade-duration-label" value={String(settings.fadeDurationMs)} disabled={busy || !showDuration} options={[150,250,400,600,...(![150,250,400,600].includes(settings.fadeDurationMs)?[settings.fadeDurationMs]:[])].map(ms=>({value:String(ms),label:`${ms} 毫秒`}))} onValueChange={value=>void change({fadeDurationMs:Number(value)})}/>
            <p className="text-muted">无缝播放开启时，自然连播保留衔接，淡入淡出用于手动切歌、暂停和恢复。</p>
          </div>}
        </div>
      </div>
    </div>
    {error&&<ErrorNotice message={error}/>}{!settings&&error&&<button className="btn btn-secondary btn-sm" onClick={()=>void read()}><RefreshCw size={15}/><span>重新读取</span></button>}
  </div>;
}
