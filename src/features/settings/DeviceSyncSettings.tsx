import { SyncResultDetails } from "./SyncResultDetails";
import { useEffect, useState } from "react";
import { Cloud } from "lucide-react";
import { callCommand, normalizeAppError } from "../../lib/api";
import { ErrorNotice } from "../../components/ErrorNotice";
import { Select } from "../../components/Select";
import { SettingsDisclosure } from "./SettingsDisclosure";

const stores = { github: "GitHub Gist", onedrive: "OneDrive", webdav: "WebDAV", supabase: "Supabase" } as const;
type Provider = keyof typeof stores;
type Status = import("../../lib/deviceSync").DeviceSyncStatus;
const request = (action: string, input: unknown = null, provider?: Provider) => callCommand<Status>("device_sync_command", {action, input, provider});
function choiceText(value: unknown) {
  if (value === null) return "已删除";
  if (typeof value === "boolean") return value ? "已收藏" : "未收藏";
  if (typeof value === "object" && value && "name" in value && "items" in value) return `${String(value.name)} · ${Array.isArray(value.items) ? value.items.length : 0} 首`;
  const record = value as {positionMs: number; completed?: boolean; note?: string};
  const seconds = Math.floor(record.positionMs / 1000);
  return [`${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, "0")}`, record.completed && "已听完", record.note].filter(Boolean).join(" · ");
}
export function DeviceSyncSettings({active, onSynced}: {active: boolean; onSynced: () => Promise<void>}) {
  const [status, setStatus] = useState<Status>();
  const [githubCode,setGithubCode] = useState("");
  const [useToken,setUseToken] = useState(false);
  async function startGithub(){
    setBusy("githubStart");setError("");setMessage("");
    try {setGithubCode(await callCommand<string>("device_sync_command",{action:"githubStart",provider:"github"}));}
    catch(e){setError(normalizeAppError(e).message);}
    finally{setBusy("");}
  }
  async function cancelGithub(){
    await callCommand("device_sync_command",{action:"githubCancel",provider:"github"}).catch(e=>setError(normalizeAppError(e).message));
    setGithubCode("");
  }
  const [dav, setDav] = useState({url:"",username:"",password:""});
  const [token, setToken] = useState("");
  const [email,setEmail] = useState("");
  const [password,setPassword] = useState("");
  const [busy, setBusy] = useState("");
  const [message, setMessage] = useState("");
  const [error, setError] = useState("");
  useEffect(() => {
    if (!active || busy) return;
    let live = true;
    const refresh = () => { void request("status").then(s => {if(live) setStatus(s);}).catch(e => {if(live)setError(normalizeAppError(e).message);}); };
    refresh();
    const timer = setInterval(refresh, 4000);
    return () => {live = false; clearInterval(timer);};
  }, [active,busy]);
  async function run(action: string, input?: unknown) {
    if (busy || status?.phase) return;
    setBusy(action); setError(""); setMessage("");
    try {
      const s = await request(action,input,status?.provider);
      if (action === "select" || action === "login") {setGithubCode("");setUseToken(false);setToken("");setPassword("");setDav(v=>({...v,password:""}));}
      setStatus(s);
      if (action === "login" || action === "sync") {
        let result = s;
        if (action === "login") {
          setBusy("sync");
          result = await request("sync", null, s.provider);
          setStatus(result);
        }
        setMessage(`同步完成，共 ${result.records} 条记录，${result.pending} 条待匹配。`);
        await onSynced();
      } else if (action === "resolve") await onSynced();
    } catch(e) {setError(normalizeAppError(e).message);} finally {if(action==="login")setGithubCode("");setBusy("");}
  }
  const selected = status?.provider ?? "github";
  const store = stores[selected];
  const needsConfig = status?.configured === false;
  return <section className="settings-card settings-group-card" aria-label="跨设备同步" aria-busy={!!busy}>
    <div className="settings-group-header">
      <div className="settings-group-title-wrap">
        <span className="settings-group-icon"><Cloud size={18} aria-hidden="true" /></span>
        <div>
          <h3>跨设备同步</h3>
          <p className="settings-group-desc">两端选择相同的存储方式并绑定同一账号，同步作品喜欢、音轨书签、歌单、收听进度和时间标记。</p>
        </div>
      </div>
    </div>
    <div className="settings-group-body">
      <div className="settings-row">
        <label htmlFor="sync-store">存储方式</label>
        <Select id="sync-store" value={selected} disabled={!!busy || !!status?.phase || !status}
          options={Object.entries(stores).map(([value,label]) => ({value,label}))}
          onValueChange={value => void run("select", value)}/>
      </div>
      {selected === "supabase" && !status?.connected && <>
        <div className="settings-row"><label htmlFor="sync-mail">邮箱</label>
          <input className="input-text" id="sync-mail" type="email" autoComplete="username" disabled={!!busy || !!status?.phase} value={email} onChange={e=>setEmail(e.target.value)}/></div>
        <div className="settings-row"><label htmlFor="sync-password">密码</label>
          <input className="input-text" id="sync-password" type="password" autoComplete="current-password" disabled={!!busy || !!status?.phase} value={password} onChange={e=>setPassword(e.target.value)}/></div>
      </>}
      {selected === "webdav" && !status?.connected && (["url","username","password"] as const).map(field =>
        <div className="settings-row" key={field}>
          <label htmlFor={`dav-${field}`}>{{url:"HTTPS 文件夹地址",username:"用户名",password:"密码或应用密码"}[field]}</label>
          <input id={`dav-${field}`} className="input-text" type={field==="password"?"password":field==="url"?"url":"text"} autoComplete="off"
            disabled={!!busy || !!status?.phase} value={dav[field]} onChange={e=>setDav({...dav,[field]:e.target.value})}/>
        </div>)}
      {selected === "github" && useToken && !status?.connected && <div className="settings-row">
        <div className="settings-label-block">
          <label htmlFor="gist-token">GitHub 访问令牌</label>
          <span className="settings-field-hint" id="gist-token-hint">使用有 Gist 读写权限的令牌。</span>
        </div>
        <input id="gist-token" type="password" autoComplete="off" className="input-text"
          aria-describedby="gist-token-hint" disabled={!!busy || !!status?.phase} value={token} onChange={e => setToken(e.target.value)}/>
      </div>}
      {needsConfig && !status.connected && <div className="settings-row">
        <span>{store} 账号</span>
        <span className="settings-field-hint">此版本暂未启用 {store} 登录。</span>
      </div>}
      {(!needsConfig || status?.connected) && <div className="settings-row">
        <div className="settings-label-block">
          <span>{store} 账号</span>
          <span className="settings-field-hint">{status ? (status.connected ? (needsConfig ? "已登录，此版本暂未启用同步" : "已登录") : "未登录") : (error ? "无法读取登录状态" : "正在读取登录状态…")}</span>
        </div>
        {status && <div className="track-actions">
          {status.connected ? <>
            <button type="button" className="btn btn-primary btn-sm" disabled={!!busy || !!status?.phase || needsConfig} onClick={() => void run("sync")}>
              {busy === "sync" ? "正在同步…" : "立即同步"}
            </button>
            <button type="button" className="btn btn-ghost btn-sm" disabled={!!busy || !!status?.phase} onClick={() => void run("logout")}>退出此设备</button>
          </> : selected==="supabase" ? <>
            <button type="button" className="btn btn-primary btn-sm" disabled={!!busy || !!status?.phase || !email.trim() || !password} onClick={()=>void run("login",{email,password})}>{busy==="login" ? "正在连接…" : "登录并同步"}</button>
            <button type="button" className="btn btn-ghost btn-sm" disabled={!!busy || !!status?.phase || !email.trim() || !password} onClick={()=>void run("login",{email,password,signup:true})}>注册账号</button>
          </> : selected==="github" && !useToken ? <>
            {githubCode ? <>
              <span>GitHub 授权码 <strong>{githubCode}</strong></span>
              <button type="button" className="btn btn-primary btn-sm" disabled={!!busy || !!status?.phase} onClick={()=>{
                void navigator.clipboard?.writeText(githubCode).catch(()=>{});
                void run("login",{device:true});
              }}>{busy==="login" ? "等待 GitHub 授权…" : "打开 GitHub 完成授权"}</button>
              <button type="button" className="btn btn-ghost btn-sm" onClick={()=>void cancelGithub()}>取消登录</button>
            </> : <button type="button" className="btn btn-primary btn-sm" disabled={!!busy || !!status?.phase} onClick={()=>void startGithub()}>登录 GitHub 并同步</button>}
          </> : <button type="button" className="btn btn-primary btn-sm" disabled={!!busy || !!status?.phase || (selected === "github" && !token.trim()) || (selected === "webdav" && !(dav.url.trim() && dav.username.trim() && dav.password))} onClick={() => void run("login", selected === "github" ? {token} : selected==="webdav" ? dav : null)}>
            {busy === "login" ? (selected === "github" || selected === "webdav" ? "正在连接…" : "等待浏览器授权…") : selected === "github" ? "绑定 GitHub 并同步" : selected==="webdav" ? "连接 WebDAV 并同步" : `登录 ${store} 并同步`}
          </button>}
        </div>}
      </div>}
      {selected==="github" && !status?.connected && !githubCode && <div className="settings-row">
        <span className="settings-field-hint">知道秘密 Gist 链接的人可以读取同步内容。</span>
        <button type="button" className="btn btn-ghost btn-sm" disabled={!!busy || !!status?.phase} onClick={()=>{setUseToken(!useToken);setToken("");}}>
          {useToken ? "使用浏览器登录" : "使用访问令牌"}</button>
      </div>}
      {status?.lastSync && <div className="settings-row">
        <span>上次同步</span>
        <span className="settings-field-hint tabular-nums">{new Date(status.lastSync).toLocaleString()} · {status.pending} 条待匹配</span>
      </div>}
      {status?.phase && <p role="status" className="settings-save-note">{status.phase}</p>}
      {status && <SyncResultDetails status={status} />}
      {message && <p className="settings-save-note" role="status">{message}</p>}
      {!!status?.metadataPending && <p className="settings-save-note" role="status">
        {status.metadataPending} 部作品的来源信息待补全。{status.metadataError} 下次同步时将重试。
      </p>}
      {(error || status?.autoError) && <ErrorNotice message={error || status?.autoError || ""}/>}
      {status?.connected && <SettingsDisclosure title="自动同步">
        {(["foreground", "periodic"] as const).map(key => <label className="settings-row" key={key}>
          <span>{key === "foreground" ? "回到应用时同步" : "后台定期同步"}</span>
          <input type="checkbox" className="toggle-switch" checked={status.autoPolicy[key]} disabled={!!busy || !!status?.phase}
            onChange={e => void run("autoPolicy", {...status.autoPolicy, [key]: e.target.checked})}/>
        </label>)}
        {status.autoPolicy.periodic && <div className="settings-row">
          <label htmlFor="drive-sync-interval">同步间隔</label>
          <Select id="drive-sync-interval" value={String(status.autoPolicy.intervalMinutes)} disabled={!!busy || !!status?.phase}
            options={[15, 30, 60, 180, 360].map(n => ({value: String(n), label: `${n} 分钟`}))}
            onValueChange={v => void run("autoPolicy", {...status.autoPolicy, intervalMinutes: Number(v)})}/>
        </div>}
      </SettingsDisclosure>}
      {!!status?.conflicts.length && <SettingsDisclosure title="处理并发修改" defaultOpen>
        {status.conflicts.map(c => <div className="settings-row" key={c.key}>
          <span>{c.label}</span>
          <div className="track-actions drive-sync-choices">{c.choices.map(v =>
            <button type="button" key={v.device + v.seq} className="btn btn-secondary btn-sm" disabled={!!busy || !!status?.phase}
              onClick={() => void run("resolve", {key: c.key, device: v.device, seq: v.seq})}>
              {typeof v.value === "object" && v.value && "deviceName" in v.value ? String(v.value.deviceName) : `设备 ${v.device.slice(0, 8)}`} · 采用 {choiceText(v.value)}
            </button>
          )}</div>
        </div>)}
      </SettingsDisclosure>}
    </div>
  </section>;
}
