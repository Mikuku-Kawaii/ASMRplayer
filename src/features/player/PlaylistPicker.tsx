import { useEffect, useRef, useState } from "react";
import { Check, ListPlus, Plus, X } from "lucide-react";
import { ContentDialog, dialogAutofocus } from "../../components/ContentDialog";
import { ErrorNotice } from "../../components/ErrorNotice";
import { Toast } from "../../components/Toast";
import { normalizeAppError } from "../../lib/api";
import { markPlaylistUsed, playlistCommand, recentPlaylists, requestPlaylist, type Playlist, type PlaylistTarget } from "./playlists";

export function PlaylistMenuItem({ target, onClose }: { target: PlaylistTarget; onClose: () => void }) {
  return <button type="button" role="menuitem" className="m3-menu-item" onClick={() => { onClose(); requestPlaylist(target); }}><ListPlus size={16} /><span>{"versionIds" in target ? "加入音轨歌单" : "加入作品集"}</span></button>;
}
interface Addition { id: string; name: string; added: string[]; works: boolean }
export function PlaylistPickerHost() {
  const [target, setTarget] = useState<PlaylistTarget | null>(null);
  const [result,setResult] = useState<Addition | null>(null);
  const [error,setError] = useState("");
  useEffect(() => { const receive = (event: Event) => {setResult(null);setTarget((event as CustomEvent<PlaylistTarget>).detail);}; window.addEventListener("playlists:add", receive); return () => window.removeEventListener("playlists:add", receive); }, []);
  async function undo(change: Addition) {
    setResult(null);
    try { await playlistCommand("changeMembers",{id:change.id,remove:change.added,...(change.works?{workItems:[]}:{})}); }
    catch(e) {setError(normalizeAppError(e).message);}
  }
  return <>{target && <PlaylistPicker key={JSON.stringify(target)} target={target} onClose={()=>setTarget(null)} onAdded={added=>{setResult(added);setTarget(null);}} />}
    {result && <Toast message={`已加入「${result.name}」`} duration={6000} onClose={()=>setResult(null)}>{!!result.added.length && <button className="btn btn-ghost btn-sm" onClick={()=>void undo(result)}>撤销</button>}</Toast>}
    {error && <Toast message={error} tone="error" onClose={()=>setError("")}/>}</>;
}
function PlaylistPicker({ target,onClose,onAdded }: {target:PlaylistTarget;onClose:()=>void;onAdded:(value:Addition)=>void}) {
  const [groups,setGroups]=useState<Playlist[]>([]), [ids,setIds]=useState<string[]>([]);
  const [name,setName]=useState(""), [query,setQuery]=useState(""), [busy,setBusy]=useState(false), [error,setError]=useState(""), [attempt,retry]=useState(0);
  const gate=useRef(false);
  const works=!("versionIds" in target);
  const label=works?"作品集":"音轨歌单";
  useEffect(()=>{
    let live=true;
    setError("");
    const items="versionIds" in target?Promise.resolve(target.versionIds):"workId" in target?Promise.resolve([target.workId]):
      playlistCommand<string>("cacheWork",{sourceWorkId:target.sourceWorkId}).then(id=>[id]);
    void Promise.all([playlistCommand<Playlist[]>("groups"),items]).then(([lists,items])=>{
      if(!live)return;
      const recent=recentPlaylists(),rank=(id:string)=>{const i=recent.indexOf(id);return i<0?Infinity:i;};
      setGroups(lists.filter(p=>(p.workItems!=null)===works).sort((a,b)=>rank(a.id)-rank(b.id)));setIds(items);
    }).catch(e=>{if(live)setError(normalizeAppError(e).message);});
    return ()=>{live=false;};
  },[target,works,attempt]);
  async function add(p?:Playlist) {
    if(gate.current || !ids.length)return;
    gate.current=true;setBusy(true);setError("");
    try {
      const items="versionIds" in target?{versionIds:ids}:{workItems:ids.map(id=>({id,directory:target.directory,format:target.format}))};
      const group=p??await playlistCommand<Playlist>("create",{name:name.trim(),...items});
      const added=p?await playlistCommand<string[]>("changeMembers",{id:p.id,...items}):ids;
      markPlaylistUsed(group.id);onAdded({id:group.id,name:group.name,added,works});
    } catch(e) {setError(normalizeAppError(e).message);} finally {gate.current=false;setBusy(false);}
  }
  return <ContentDialog label={`加入${label}`} onClose={onClose} busy={busy}>
    <header className="modal-header"><h3>加入{label}</h3><button className="btn-icon" aria-label="关闭" onClick={onClose} disabled={busy}><X size={18}/></button></header>
    <div className="dialog-body"><input ref={dialogAutofocus} className="input-text" type="search" value={query} onChange={e=>setQuery(e.target.value)} aria-label={`搜索${label}`} placeholder={`搜索${label}`}/>
      {!ids.length && !error && <p role="status">正在读取…</p>}
      <div className="playlist-picker-groups">{groups.filter(g=>g.name.toLocaleLowerCase().includes(query.trim().toLocaleLowerCase())).map(g=>{
        const members=g.workItems?.map(w=>w.id)??g.versionIds;const included=ids.length>0 && ids.every(id=>members.includes(id));
        return <button className="playlist-picker-group" key={g.id} disabled={busy || !ids.length || included} onClick={()=>void add(g)}>
          <span><strong>{g.name}</strong><small>{members.length} {works?"部作品":"首音轨"}</small></span>{included?<Check size={18} aria-label="已加入"/>:<ListPlus size={18}/>}</button>;
      })}</div>
      <form className="playlist-name-form" onSubmit={e=>{e.preventDefault();void add();}}><label htmlFor="playlist-picker-name">新建{label}</label><div className="playlist-inline-form"><input id="playlist-picker-name" className="input-text" maxLength={80} value={name} onChange={e=>setName(e.target.value)} placeholder="名称"/><button className="btn btn-primary btn-sm" disabled={busy || !name.trim() || !ids.length}><Plus size={15}/>新建并加入</button></div></form>
      {error && <><ErrorNotice message={error}/><button className="btn btn-ghost" onClick={()=>retry(n=>n+1)}>重试</button></>}
    </div></ContentDialog>;
}
