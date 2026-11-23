import { useCallback, useEffect, useRef, useState } from "react";
import { ArrowDown, ArrowLeft, ArrowUp, ListMusic, ListPlus, Pencil, Play, Plus, Trash2, X } from "lucide-react";
import { ContentDialog, dialogAutofocus } from "../../components/ContentDialog";
import { ErrorNotice } from "../../components/ErrorNotice";
import { Toast } from "../../components/Toast";
import { formatDurationMs, normalizeAppError, type PlaybackSnapshot } from "../../lib/api";
import { markPlaylistUsed, playlistCommand, requestPlaylist, watchPlaylists, type CollectionCard, type Playlist, type PlaylistTrack } from "./playlists";

interface Choice { id: string; title: string; detail?: string; coverRef?: string | null }
function Artwork({ items }: { items: {coverRef?: string | null}[] }) {
  const covers = [...new Set(items.flatMap(item => item.coverRef ? [item.coverRef] : []))].slice(0, 4);
  return <div className={`playlist-artwork ${covers.length > 1 ? "mosaic" : ""}`} aria-hidden="true">
    {covers.length ? covers.map(ref => <img key={ref} src={ref} alt="" referrerPolicy="no-referrer" />) : <ListMusic size={32} />}
  </div>;
}
export function MemberPicker({ title, choices, existing = [], initial = [], name, loading, error, busy, onClose, onSubmit, verb = "添加", max = 500 }: {
  title: string; choices: Choice[]; existing?: string[]; initial?: string[]; name?: string; loading?: boolean; error?: string; busy?: boolean; max?:number;
  onClose: () => void; onSubmit: (ids: string[], name: string) => void; verb?: string;
}) {
  const [query,setQuery] = useState(""), [selected,setSelected] = useState<string[]>(initial), [newName,setName] = useState(name ?? "");
  const visible = choices.filter(c => (c.title+" "+(c.detail ?? "")).toLocaleLowerCase().includes(query.trim().toLocaleLowerCase()));
  return <ContentDialog label={title} busy={busy} onClose={onClose}>
    <header className="modal-header"><h3>{title}</h3><button className="btn-icon" aria-label="关闭" disabled={busy} onClick={onClose}><X size={18}/></button></header>
    <div className="dialog-body">
      {name !== undefined && <label className="playlist-name-form">新作品集名称<input className="input-text" maxLength={80} value={newName} onChange={e=>setName(e.target.value)}/></label>}
      <input ref={dialogAutofocus} className="input-text" type="search" aria-label="搜索条目" placeholder="搜索名称" value={query} onChange={e=>setQuery(e.target.value)} />
      {loading && <p role="status">正在读取…</p>}{error && <ErrorNotice message={error}/>}
      <div className="playlist-track-picker">{visible.map(c => {
        const included=existing.includes(c.id), checked=selected.includes(c.id);
        return <label key={c.id}><input type="checkbox" checked={included || checked} disabled={busy || included || (!checked && selected.length + existing.length >= max)} onChange={()=>setSelected(ids=>checked ? ids.filter(id=>id!==c.id) : [...ids,c.id])}/>
          <span><strong>{c.title}</strong><small>{included ? "已加入" : c.detail}</small></span></label>;
      })}{!loading && !visible.length && <p className="text-muted">没有符合条件的条目</p>}</div>
    </div>
    <footer className="modal-actions"><button className="btn btn-ghost" disabled={busy} onClick={onClose}>取消</button>
      <button className="btn btn-primary" disabled={busy || !selected.length || (name !== undefined && !newName.trim())}
        onClick={()=>onSubmit(choices.filter(c=>selected.includes(c.id)).map(c=>c.id),newName.trim())}>{verb}（{selected.length}）</button></footer>
  </ContentDialog>;
}
export function originWorks(tracks: PlaylistTrack[]): Choice[] {
  const groups = new Map<string, {track: PlaylistTrack; chapters: Set<string>}>();
  for (const t of tracks) {
    if (!t.workId || !t.trackId) continue;
    const g = groups.get(t.workId) ?? {track:t,chapters:new Set<string>()};
    g.chapters.add(t.trackId); groups.set(t.workId,g);
  }
  return [...groups].map(([id,{track,chapters}])=>({id,title:track.workTitle,detail:`已收 ${chapters.size} / ${track.workTrackCount ?? chapters.size} 轨`}));
}
function trackChoice(track: PlaylistTrack): Choice {
  return {id:track.versionId,title:track.title,coverRef:track.coverRef,detail:[track.workTitle,track.versionLabel,track.durationMs ? formatDurationMs(track.durationMs) : "",!track.available ? "暂不可用" : ""].filter(Boolean).join(" · ")};
}
function workChoice(work:CollectionCard): Choice {
  const progress=work.completed ? "已听完" : work.lastPositionMs ? `听到 ${formatDurationMs(work.lastPositionMs)}` : "未收听";
  return {...work,detail:work.available ? progress : "暂不可用"};
}
function LibraryMemberPicker({group,busy,error,onClose,onAdd}:{group:Playlist;busy:boolean;error:string;onClose:()=>void;onAdd:(ids:string[])=>void}) {
  const [choices,setChoices]=useState<Choice[]>([]),[loading,setLoading]=useState(true),[failure,setFailure]=useState("");
  const works=group.workItems!=null;
  useEffect(()=>{
    let live=true;
    const list=works ? playlistCommand<Choice[]>("works") : playlistCommand<PlaylistTrack[]>("tracks").then(rows=>rows.map(trackChoice));
    void list.then(rows=>{if(live)setChoices(rows);}).catch(e=>{if(live)setFailure(normalizeAppError(e).message);}).finally(()=>{if(live)setLoading(false);});
    return ()=>{live=false;};
  },[works]);
  return <MemberPicker title={works?"添加作品":"添加音轨"} choices={choices} existing={group.workItems?.map(w=>w.id) ?? group.versionIds} max={works ? Infinity : 500} busy={busy} loading={loading} error={error || failure} onClose={onClose} onSubmit={onAdd}/>;
}
export function Playlists({onSnapshotChange,onOpenWork}:{onSnapshotChange:(value:PlaybackSnapshot)=>void;onOpenWork?:(id:string)=>void}) {
  const [groups,setGroups]=useState<Playlist[]>([]),[ready,setReady]=useState(false),[works,setWorks]=useState(true),[selected,setSelected]=useState<string | null>(null);
  const [busy,setBusy]=useState(false),[error,setError]=useState(""),[notice,setNotice]=useState("");
  const [dialog,setDialog]=useState<"name"|"remove"|"add"|"convert"|null>(null),[name,setName]=useState(""),[editing,setEditing]=useState(false);
  const [undo,setUndo]=useState<null | {id:string; works:boolean; added:string[]}>(null);
  const epoch=useRef(0),pending=useRef(false),drag=useRef<string | null>(null);
  const reload=useCallback(async()=>{
    const token=++epoch.current;
    try {const rows=await playlistCommand<Playlist[]>("list");if(token===epoch.current){setGroups(rows);setReady(true);}}
    catch(e){if(token===epoch.current)setError(normalizeAppError(e).message);}
  },[]);
  useEffect(()=>{void reload();const stop=watchPlaylists(()=>void reload());return stop;},[reload]);
  const group=groups.find(g=>g.id===selected), label=works?"作品集":"音轨歌单", unit=works?"部作品":"首音轨";
  const lists=groups.filter(g=>(g.workItems!=null)===works);
  const cards=group?.works ?? [], tracks=group?.tracks ?? [];
  const rows=works ? cards.map(workChoice) : tracks.map(trackChoice);
  const ids=group?.workItems?.map(w=>w.id) ?? group?.versionIds ?? [];
  function open(id:string|null) {setSelected(id);setEditing(false);setDialog(null);setError("");if(id)markPlaylistUsed(id);}
  async function run<T = Playlist>(action:string,input:Record<string,unknown>,done?:(value:T)=>void) {
    if(pending.current)return;
    pending.current=true;setBusy(true);setError("");setUndo(null);setNotice("");
    try {
      if(action==="play" || action==="append") {
        onSnapshotChange(await playlistCommand<PlaybackSnapshot>(action,input));
        if(action==="append")setNotice("已加入播放队列");
      } else {
        const value=await playlistCommand<T>(action,input);await reload();done?.(value);
      }
    } catch(e){setError(normalizeAppError(e).message);}
    finally{setBusy(false);pending.current=false;}
  }
  function move(id:string,target:number) {
    if(group && ids.indexOf(id)!==target)void run("changeMembers",{id:group.id,...(works?{workItems:[]}:{}),itemId:id,target,expected:ids});
  }
  const playable=(p:Playlist)=>(p.workItems!=null?p.works:p.tracks)?.some(row=>row.available);
  const origins=dialog==="convert"?originWorks(tracks):[];
  return <section className="playlists-panel playlists-page" aria-label="歌单" aria-busy={busy}>
    <div className="playlist-toolbar">{group?<button className="btn btn-ghost" onClick={()=>open(null)}><ArrowLeft size={16}/>{label}</button>:<h2>歌单</h2>}
      <span className="playlist-toolbar-spacer"/>
      {!group && <button className="btn btn-primary" disabled={busy} onClick={()=>{setName("");setDialog("name");}}><Plus size={16}/>新建{label}</button>}</div>
    {!dialog && error && <ErrorNotice message={error}/>}
    {!group && <><div className="playlist-tabs" role="group" aria-label="歌单类型">{[true,false].map(value=><button key={String(value)} aria-pressed={value===works} onClick={()=>{setWorks(value);open(null);}}>{value?"作品集":"音轨歌单"}</button>)}</div>
      {!ready && !error && <p role="status">正在读取歌单…</p>}{ready && !lists.length && <div className="empty-box">{works?"还没有作品集，可从音频库或在线浏览加入整部作品。":"还没有音轨歌单，可从作品详情挑选音轨。"}</div>}
      <div className="playlist-grid">{lists.map(p=><div className="playlist-tile" key={p.id}>
        <button className="playlist-tile-open" onClick={()=>open(p.id)} aria-label={`打开${label} ${p.name}`}><Artwork items={works?p.works ?? []:p.tracks}/><strong title={p.name}>{p.name}</strong><small>{p.workItems?.length ?? p.versionIds.length} {unit}</small></button>
        <button className="btn-icon" disabled={busy || !playable(p)} aria-label={`播放${label} ${p.name}`} onClick={()=>void run("play",{id:p.id})}><Play size={18}/></button>
      </div>)}</div></>}
    {group && <><div className="playlist-summary"><Artwork items={works?cards:tracks}/><div><h2>{group.name}</h2><p>{ids.length} {unit}{!works && tracks.some(t=>!t.available) ? ` · ${tracks.filter(t=>!t.available).length} 首暂不可用` : ""}</p></div></div>
      <div className="playlist-actions">
        <button className="btn btn-primary" disabled={busy || !playable(group)} onClick={()=>void run("play",{id:group.id})}><Play size={16}/>播放全部</button>
        <button className="btn btn-secondary" disabled={busy || !playable(group)} onClick={()=>void run("append",{id:group.id})}><ListPlus size={16}/>加入队列</button>
        <button className="btn btn-secondary" disabled={busy} onClick={()=>setDialog("add")}><Plus size={16}/>{works?"添加作品":"添加音轨"}</button>
        {!works && <button className="btn btn-ghost" disabled={busy || !tracks.some(t=>t.workId)} onClick={()=>setDialog("convert")}>整理为作品集</button>}
        <button className="btn btn-ghost" disabled={busy} aria-pressed={editing} onClick={()=>setEditing(!editing)}><Pencil size={15}/>{editing?"完成":"编辑"}</button>
      </div>
      {editing && <div className="playlist-actions"><button className="btn btn-ghost btn-sm" onClick={()=>{setName(group.name);setDialog("name");}}>重命名</button>
        {[-1,1].map(delta=><button className="btn btn-ghost btn-sm" key={delta} disabled={busy || !lists[lists.indexOf(group)+delta]} onClick={()=>void run("update",{id:group.id,groupDelta:delta})}>{delta<0?"上移":"下移"}{label}</button>)}
        <button className="btn btn-ghost btn-sm danger-text" onClick={()=>setDialog("remove")}><Trash2 size={15}/>删除{label}</button></div>}
      {!rows.length && <p className="empty-box">还没有{works?"作品":"音轨"}</p>}
      <ol className="playlist-tracks">{rows.map((row,i)=><li key={row.id} draggable={editing && !busy}
        onDragStart={()=>{drag.current=row.id;}} onDragOver={e=>{if(editing)e.preventDefault();}} onDrop={e=>{e.preventDefault();if(editing && drag.current)move(drag.current,i);drag.current=null;}} onDragEnd={()=>{drag.current=null;}}>
        <span className="playlist-track-number">{i+1}</span>
        {works && row.coverRef && <img className="playlist-work-thumb" alt="" src={row.coverRef} referrerPolicy="no-referrer"/>}
        <button className="playlist-track-open" disabled={busy || (works ? !(cards[i]?.openable ?? cards[i]?.available) || !onOpenWork : !tracks[i]?.available)}
          onClick={()=>works ? onOpenWork?.(row.id) : void run("play",{id:group.id,startVersionId:row.id})}><strong title={row.title}>{row.title}</strong><small>{row.detail}</small></button>
        {editing ? <div className="playlist-track-tools">{[-1,1].map(delta=><button className="btn-icon" aria-label={`${delta<0?"上移":"下移"} ${row.title}`} disabled={busy || !rows[i+delta]} key={delta} onClick={()=>move(row.id,i+delta)}>{delta<0?<ArrowUp size={15}/>:<ArrowDown size={15}/>}</button>)}
          <button className="btn-icon" aria-label={`移出 ${row.title}`} disabled={busy} onClick={()=>void run("changeMembers",{id:group.id,remove:[row.id],...(works?{workItems:[]}:{})})}><X size={15}/></button></div>
          : !works && tracks[i]?.available && <button className="btn-icon" aria-label={`加入音轨歌单 ${row.title}`} onClick={()=>requestPlaylist({versionIds:[row.id]})}><ListPlus size={16}/></button>}
      </li>)}</ol></>}
    {dialog==="name" && <ContentDialog size="compact" label={group?"重命名":`新建${label}`} busy={busy} onClose={()=>setDialog(null)}><form className="dialog-form" onSubmit={e=>{e.preventDefault();void run(group?"update":"create",{id:group?.id,name:name.trim(),...(!group && works?{workItems:[]}:{})},p=>open(group?.id ?? p.id));}}>
      <header className="modal-header"><h3>{group?"重命名":`新建${label}`}</h3></header><div className="dialog-body"><label className="playlist-name-form">{label}名称<input ref={dialogAutofocus} className="input-text" maxLength={80} value={name} onChange={e=>setName(e.target.value)}/></label>{error && <ErrorNotice message={error}/>}</div>
      <footer className="modal-actions"><button type="button" className="btn btn-ghost" disabled={busy} onClick={()=>setDialog(null)}>取消</button><button className="btn btn-primary" disabled={busy || !name.trim()}>保存</button></footer></form></ContentDialog>}
    {dialog==="remove" && group && <ContentDialog size="compact" label={`删除${label}`} busy={busy} onClose={()=>setDialog(null)}><header className="modal-header"><h3>删除「{group.name}」？</h3></header><div className="dialog-body">音频库中的内容会保留。{error && <ErrorNotice message={error}/>}</div><footer className="modal-actions"><button className="btn btn-ghost" disabled={busy} onClick={()=>setDialog(null)}>取消</button><button className="btn btn-danger" disabled={busy} onClick={()=>void run("delete",{id:group.id},()=>open(null))}>删除</button></footer></ContentDialog>}
    {dialog==="add" && group && <LibraryMemberPicker group={group} busy={busy} error={error} onClose={()=>setDialog(null)} onAdd={items=>void run<string[]>("changeMembers",{id:group.id,...(works?{workItems:items.map(id=>({id}))}:{versionIds:items})},added=>{
      setDialog(null);markPlaylistUsed(group.id);setNotice(`已加入「${group.name}」`);setUndo(added.length?{id:group.id,works,added}:null);
    })}/>}
    {dialog==="convert" && group && <MemberPicker title="整理为作品集" max={Infinity} name={group.name.slice(0,74)+" · 作品集"} choices={origins} initial={origins.map(w=>w.id)} verb="另存为作品集" busy={busy} error={error} onClose={()=>setDialog(null)}
      onSubmit={(items,newName)=>void run("create",{name:newName,workItems:items.map(id=>({id}))},p=>{setWorks(true);open(p.id);setNotice("已另存为作品集，原音轨歌单已保留");})}/>}
    {notice && <Toast message={notice} onClose={()=>{setNotice("");setUndo(null);}}>
      {undo && <button className="btn btn-ghost btn-sm" onClick={()=>void run("changeMembers",{id:undo.id,remove:undo.added,...(undo.works?{workItems:[]}:{})},()=>{setNotice("");setUndo(null);})}>撤销</button>}
    </Toast>}
  </section>;
}
