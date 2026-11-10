import { useEffect, useRef, useState } from "react";
import { Check, ListPlus, Plus, X } from "lucide-react";
import { ContentDialog } from "../../components/ContentDialog";
import { ErrorNotice } from "../../components/ErrorNotice";
import { Toast } from "../../components/Toast";
import { normalizeAppError } from "../../lib/api";
import { playlistCommand, requestPlaylist, type Playlist, type PlaylistTarget } from "./playlists";

export function PlaylistMenuItem({ target, onClose }: { target: PlaylistTarget; onClose: () => void }) {
  return <button type="button" role="menuitem" className="m3-menu-item" onClick={() => { onClose(); requestPlaylist(target); }}><ListPlus size={16} aria-hidden="true" /><span className="ui-icon-label">加入歌单</span></button>;
}
export function PlaylistPickerHost() {
  const [target, setTarget] = useState<PlaylistTarget | null>(null);
  useEffect(() => { const receive = (event: Event) => setTarget((event as CustomEvent<PlaylistTarget>).detail); window.addEventListener("playlists:add", receive); return () => window.removeEventListener("playlists:add", receive); }, []);
  return target ? <PlaylistPicker key={JSON.stringify(target)} target={target} onClose={() => setTarget(null)} /> : null;
}
function PlaylistPicker({ target, onClose }: { target: PlaylistTarget; onClose: () => void }) {
  const [groups, setGroups] = useState<Playlist[]>([]), [ids, setIds] = useState<string[] | null>(null);
  const [name, setName] = useState(""), [error, setError] = useState(""), [busy, setBusy] = useState(false), [notice, setNotice] = useState("");
  const [retry, setRetry] = useState(0);
  const gate = useRef(false), mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    setError(""); setIds(null);
    void Promise.all([playlistCommand<Playlist[]>("list"), "versionIds" in target ? Promise.resolve(target.versionIds) : playlistCommand<string[]>("resolveWork", target)])
      .then(([groups, ids]) => { if (mounted.current) { setGroups(groups); setIds(ids); } }).catch(e => { if (mounted.current) setError(normalizeAppError(e).message); });
    return () => { mounted.current = false; };
  }, [target, retry]);
  async function add(group?: Playlist) {
    if (gate.current || !ids?.length) return;
    gate.current = true; setBusy(true); setError("");
    try {
      if (group) await playlistCommand("update", { id: group.id, versionIds: ids });
      else await playlistCommand("create", { name: name.trim(), versionIds: ids });
      const latest = await playlistCommand<Playlist[]>("list");
      if (mounted.current) { setNotice(`已加入「${group?.name ?? name.trim()}」`); setGroups(latest); setName(""); }
    } catch (e) { if (mounted.current) setError(normalizeAppError(e).message); }
    finally { gate.current = false; if (mounted.current) setBusy(false); }
  }
  return <ContentDialog label="加入歌单" onClose={onClose} busy={busy}>
    <header className="modal-header"><h3>加入歌单</h3><button type="button" className="btn-icon" disabled={busy} onClick={onClose} aria-label="关闭加入歌单" title="关闭"><X size={18} /></button></header>
    <p className="text-muted">{ids === null ? error ? "音轨读取未完成" : "正在读取优先音轨…" : ids.length ? `已选择 ${ids.length} 首音轨` : "没有可加入的音轨，请检查作品来源。"}</p>
    {error && ids === null && <button type="button" className="btn btn-ghost btn-sm" onClick={() => setRetry(value => value + 1)}>重试读取音轨</button>}
    <div className="playlist-picker-groups">{groups.map(group => {
      const included = !!ids?.length && ids.every(id => group.versionIds.includes(id));
      return <button key={group.id} type="button" className="playlist-picker-group" disabled={busy || !ids?.length || included} onClick={() => void add(group)}>
        <span><strong>{group.name}</strong><small>{group.versionIds.length} 首音轨</small></span>{included ? <Check size={19} aria-label="已加入" /> : <ListPlus size={19} aria-hidden="true" />}
      </button>;
    })}</div>
    <form className="playlist-name-form" onSubmit={event => { event.preventDefault(); void add(); }}><label htmlFor="playlist-picker-name">新建歌单</label><div className="playlist-inline-form"><input id="playlist-picker-name" className="input-text" maxLength={80} placeholder="歌单名称" value={name} disabled={busy} onChange={event => setName(event.target.value)} /><button className="btn btn-primary btn-sm" disabled={busy || !name.trim() || !ids?.length}><Plus size={15} /><span className="ui-icon-label">新建并加入</span></button></div></form>
    {error && <ErrorNotice message={error} />}{notice && <Toast message={notice} onClose={() => setNotice("")} />}
  </ContentDialog>;
}
