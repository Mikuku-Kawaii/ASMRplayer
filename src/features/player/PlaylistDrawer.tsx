import { useCallback, useEffect, useRef, useState } from "react";
import { ArrowDown, ArrowLeft, ArrowUp, ListMusic, ListPlus, Pencil, Play, Plus, RefreshCw, Trash2, X } from "lucide-react";
import { ContentDialog } from "../../components/ContentDialog";
import { ContextMenu } from "../../components/ContextMenu";
import { ErrorNotice } from "../../components/ErrorNotice";
import { Toast } from "../../components/Toast";
import { normalizeAppError, type PlaybackSnapshot } from "../../lib/api";
import { PlaylistMenuItem } from "./PlaylistPicker";
import { playlistCommand, watchPlaylists, type Playlist, type PlaylistTrack } from "./playlists";

function Artwork({ tracks }: { tracks: PlaylistTrack[] }) {
  const covers = [...new Set(tracks.map(track => track.coverRef).filter((ref): ref is string => !!ref))].slice(0, 4);
  return <div className={`playlist-artwork ${covers.length > 1 ? "mosaic" : ""}`} aria-hidden="true">{covers.length ? covers.map(ref => <img key={ref} src={ref} alt="" referrerPolicy="no-referrer" onError={event => { event.currentTarget.style.visibility = "hidden"; }} />) : <ListMusic size={32} />}</div>;
}

export function Playlists({ onSnapshotChange }: { onSnapshotChange: (value: PlaybackSnapshot) => void }) {
  const [groups, setGroups] = useState<Playlist[]>([]), [ready, setReady] = useState(false), [selected, setSelected] = useState<string | null>(null);
  const [busy, setBusy] = useState(false), [error, setError] = useState(""), [notice, setNotice] = useState("");
  const [nameDialog, setNameDialog] = useState<{ id?: string; name: string } | null>(null), [removing, setRemoving] = useState<Playlist | null>(null);
  const [picker, setPicker] = useState(false), [editing, setEditing] = useState(false), [menu, setMenu] = useState<{ x: number; y: number; version: string } | null>(null);
  const alive = useRef(true), epoch = useRef(0), pending = useRef(false);
  const reload = useCallback(async () => {
    const request = ++epoch.current;
    try { const value = await playlistCommand<Playlist[]>("list"); if (alive.current && request === epoch.current) { setGroups(value); setReady(true); setError(""); } }
    catch (e) { if (alive.current && request === epoch.current) setError(normalizeAppError(e).message); }
  }, []);
  useEffect(() => { alive.current = true; void reload(); const stop = watchPlaylists(() => void reload()); return () => { alive.current = false; stop(); }; }, [reload]);
  const group = groups.find(group => group.id === selected);
  useEffect(() => { if (ready && selected && !groups.some(group => group.id === selected)) { setSelected(null); setPicker(false); setEditing(false); setMenu(null); } }, [ready, selected, groups]);
  async function run(action: string, input: Record<string, unknown>, done?: () => void) {
    if (pending.current) return;
    pending.current = true; setBusy(true); setError("");
    try {
      const result = await playlistCommand<PlaybackSnapshot>(action, input);
      if (action === "play" || action === "append") onSnapshotChange(result);
      if (alive.current) { await reload(); done?.(); if (action === "append") setNotice("已加入播放队列"); }
    } catch (e) { if (alive.current) setError(normalizeAppError(e).message); }
    finally { pending.current = false; if (alive.current) setBusy(false); }
  }
  const available = group?.tracks.filter(track => track.available).length ?? 0;
  return <div className="drawer-body playlists-panel" aria-busy={busy}>
    <div className="playlist-toolbar">
      {group ? <button className="btn btn-ghost btn-sm" onClick={() => { setSelected(null); setEditing(false); setMenu(null); }}><ArrowLeft size={16} /><span>我的歌单</span></button> : <h4>我的歌单</h4>}
      <span className="playlist-toolbar-spacer" />
      {!group && <button className="btn btn-primary btn-sm" disabled={busy} onClick={() => setNameDialog({ name: "" })}><Plus size={15} /><span>新建歌单</span></button>}
      <button className="btn-icon" title="刷新歌单" aria-label="刷新歌单" disabled={busy} onClick={() => void reload()}><RefreshCw size={16} /></button>
    </div>
    {error && <ErrorNotice message={error} />}
    {!ready && !error && <p role="status" className="text-muted">正在读取歌单…</p>}
    {ready && !group && <>
      {!groups.length && <div className="empty-box">还没有歌单。新建后可从音频库挑选音轨，或右键作品加入优先音轨。</div>}
      <div className="playlist-grid">{groups.map(item => <div className="playlist-tile" key={item.id}>
        <button className="playlist-tile-open" onClick={() => setSelected(item.id)} aria-label={`打开歌单 ${item.name}`}><Artwork tracks={item.tracks} /><strong title={item.name}>{item.name}</strong><small>{item.versionIds.length} 首音轨</small></button>
        <button className="btn-icon" title={`播放歌单 ${item.name}`} aria-label={`播放歌单 ${item.name}`} disabled={busy || !item.tracks.some(track => track.available)} onClick={() => void run("play", { id: item.id })}><Play size={18} /></button>
      </div>)}</div>
    </>}
    {group && <>
      <div className="playlist-summary"><Artwork tracks={group.tracks} /><div><h4 title={group.name}>{group.name}</h4><p>{group.versionIds.length} 首音轨{available < group.versionIds.length ? ` · ${group.versionIds.length - available} 首暂不可用` : ""}</p></div></div>
      <div className="playlist-actions">
        <button className="btn btn-primary btn-sm" disabled={busy || !available} onClick={() => void run("play", { id: group.id })}><Play size={15} /><span>播放全部</span></button>
        <span className="playlist-toolbar-spacer" />
        <button className="btn-icon" title="加入队列" aria-label="加入队列" disabled={busy || !available} onClick={() => void run("append", { id: group.id })}><ListPlus size={17} /></button>
        <button className="btn-icon" title="添加音轨" aria-label="添加音轨" disabled={busy} onClick={() => setPicker(true)}><Plus size={17} /></button>
        <button className="btn btn-ghost btn-sm" disabled={busy} aria-pressed={editing} onClick={() => setEditing(value => !value)}><Pencil size={15} /><span>{editing ? "完成" : "编辑"}</span></button>
      </div>
      {editing && <div className="playlist-actions playlist-edit-actions">
        <button className="btn btn-ghost btn-sm" disabled={busy} onClick={() => setNameDialog({ id: group.id, name: group.name })}>重命名</button>
        <button className="btn-icon" title="上移歌单" aria-label="上移歌单" disabled={busy || groups.indexOf(group) === 0} onClick={() => void run("update", { id: group.id, groupDelta: -1 })}><ArrowUp size={16} /></button>
        <button className="btn-icon" title="下移歌单" aria-label="下移歌单" disabled={busy || groups.indexOf(group) === groups.length - 1} onClick={() => void run("update", { id: group.id, groupDelta: 1 })}><ArrowDown size={16} /></button>
        <button className="btn btn-ghost btn-sm danger-text" disabled={busy} onClick={() => setRemoving(group)}><Trash2 size={15} /><span>删除歌单</span></button>
      </div>}
      {!group.tracks.length && <div className="empty-box">还没有音轨，从音频库选几首加入吧。</div>}
      <ol className="playlist-tracks">{group.tracks.map((track, index) => <li key={track.versionId} onContextMenu={event => { event.preventDefault(); setMenu({ x: event.clientX, y: event.clientY, version: track.versionId }); }}>
        <span className="playlist-track-number">{index + 1}</span><button className="playlist-track-open" disabled={busy || !track.available} onClick={() => void run("play", { id: group.id, startVersionId: track.versionId })}><strong title={track.title}>{track.title}</strong><small title={track.workTitle}>{track.workTitle}{!track.available && track.workTitle ? " · 暂不可用" : ""}</small></button>
        {editing && <div className="playlist-track-tools"><button className="btn-icon" disabled={busy || index === 0} title="上移音轨" aria-label={`上移音轨 ${track.title}`} onClick={() => void run("update", { id: group.id, versionId: track.versionId, target: index - 1, expected: group.versionIds })}><ArrowUp size={15} /></button><button className="btn-icon" disabled={busy || index === group.tracks.length - 1} title="下移音轨" aria-label={`下移音轨 ${track.title}`} onClick={() => void run("update", { id: group.id, versionId: track.versionId, target: index + 1, expected: group.versionIds })}><ArrowDown size={15} /></button><button className="btn-icon" disabled={busy} title="移出歌单" aria-label={`移出歌单 ${track.title}`} onClick={() => void run("update", { id: group.id, versionId: track.versionId })}><X size={15} /></button></div>}
      </li>)}</ol>
    </>}
    {menu && <ContextMenu x={menu.x} y={menu.y} label="歌单音轨操作" onClose={() => setMenu(null)}><PlaylistMenuItem target={{ versionIds: [menu.version] }} onClose={() => setMenu(null)} /></ContextMenu>}
    {nameDialog && <ContentDialog size="compact" label={nameDialog.id ? "重命名歌单" : "新建歌单"} busy={busy} onClose={() => setNameDialog(null)}><form onSubmit={event => { event.preventDefault(); void run(nameDialog.id ? "update" : "create", { id: nameDialog.id, name: nameDialog.name.trim() }, () => setNameDialog(null)); }} className="dialog-form"><header className="modal-header"><h3>{nameDialog.id ? "重命名歌单" : "新建歌单"}</h3><button type="button" className="btn-icon" title="关闭" aria-label="关闭歌单名称" disabled={busy} onClick={() => setNameDialog(null)}><X size={18} /></button></header>
<div className="dialog-body"><label className="playlist-name-form">歌单名称<input autoFocus className="input-text" maxLength={80} value={nameDialog.name} disabled={busy} onChange={event => setNameDialog({ ...nameDialog, name: event.target.value })} /></label>{error && <ErrorNotice message={error} />}</div>
<footer className="modal-actions"><button type="button" className="btn btn-ghost" disabled={busy} onClick={() => setNameDialog(null)}>取消</button><button className="btn btn-primary" disabled={busy || !nameDialog.name.trim()}>{nameDialog.id ? "保存名称" : "新建歌单"}</button></footer></form></ContentDialog>}
    {removing && <ContentDialog size="compact" label="删除歌单" busy={busy} onClose={() => setRemoving(null)}><header className="modal-header"><h3>删除歌单</h3><button className="btn-icon" disabled={busy} title="关闭" aria-label="关闭删除歌单" onClick={() => setRemoving(null)}><X size={18} /></button></header>
<div className="dialog-body"><p>删除「{removing.name}」？保留音频文件与当前播放队列。</p>{error && <ErrorNotice message={error} />}</div>
<footer className="modal-actions"><button className="btn btn-ghost" disabled={busy} onClick={() => setRemoving(null)}>取消</button><button className="btn btn-danger btn-danger-solid" disabled={busy} onClick={() => void run("delete", { id: removing.id }, () => setRemoving(null))}>删除歌单</button></footer></ContentDialog>}
    {picker && group && <TrackPicker existing={group.versionIds} busy={busy} error={error} onClose={() => setPicker(false)} onAdd={versionIds => void run("update", { id: group.id, versionIds }, () => setPicker(false))} />}
    {notice && <Toast message={notice} onClose={() => setNotice("")} />}
  </div>;
}

function TrackPicker({ existing, busy, error, onClose, onAdd }: { existing: string[]; busy: boolean; error: string; onClose: () => void; onAdd: (ids: string[]) => void }) {
  const [tracks, setTracks] = useState<PlaylistTrack[]>([]), [loading, setLoading] = useState(true), [failure, setFailure] = useState(""), [query, setQuery] = useState(""), [selected, setSelected] = useState<string[]>([]);
  const load = useCallback(async () => { setLoading(true); setFailure(""); try { setTracks(await playlistCommand<PlaylistTrack[]>("tracks")); } catch (e) { setFailure(normalizeAppError(e).message); } finally { setLoading(false); } }, []);
  useEffect(() => { void load(); }, [load]);
  const filtered = tracks.filter(track => `${track.title} ${track.workTitle}`.toLocaleLowerCase().includes(query.trim().toLocaleLowerCase()));
  return <ContentDialog label="添加音轨" busy={busy} onClose={onClose}><header className="modal-header"><h3>添加音轨</h3><button className="btn-icon" disabled={busy} title="关闭" aria-label="关闭添加音轨" onClick={onClose}><X size={18} /></button></header>
<div className="dialog-body"><input autoFocus className="input-text" aria-label="搜索音轨或作品" placeholder="搜索音轨或作品" value={query} onChange={event => setQuery(event.target.value)} />{loading && <p role="status">正在读取音轨…</p>}{failure && <><ErrorNotice message={failure} /><button className="btn btn-ghost" onClick={() => void load()}>重试读取</button></>}{error && <ErrorNotice message={error} />}<div className="playlist-track-picker">{filtered.map(track => { const included = existing.includes(track.versionId); return <label key={track.versionId}><input type="checkbox" checked={included || selected.includes(track.versionId)} disabled={busy || included || (!selected.includes(track.versionId) && existing.length + selected.length >= 500)} onChange={event => setSelected(value => event.target.checked ? [...value, track.versionId] : value.filter(id => id !== track.versionId))} /><span><strong>{track.title}</strong><small>{included ? "已在歌单中" : track.workTitle}</small></span></label>; })}{!loading && !failure && !filtered.length && <p className="text-muted">没有符合条件的可用音轨</p>}</div></div>
<footer className="modal-actions"><button className="btn btn-primary" disabled={busy || !selected.length} onClick={() => onAdd(selected)}>添加 {selected.length} 首</button></footer></ContentDialog>;
}
