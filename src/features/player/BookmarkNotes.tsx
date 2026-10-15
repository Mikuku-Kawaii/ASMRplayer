import { useCallback, useEffect, useId, useRef, useState } from "react";
import { BookmarkCheck, Download, Edit3, FolderTree, Play, Plus, Search, Trash2, Upload } from "lucide-react";
import { Select } from "../../components/Select";
import { save } from "@tauri-apps/plugin-dialog";
import { BookmarkBundle, BookmarkDto, BookmarkRestoreReport, BookmarkSearchPage, PlaybackSnapshot, callCommand, formatDurationMs, normalizeAppError } from "../../lib/api";

interface Props {
  snapshot: PlaybackSnapshot;
  onSnapshotChange: (value: PlaybackSnapshot) => void;
  onOpenWorkDetail?: (workId: string, versionId?: string) => void;
}
const splitTags = (text: string) => text.split(/[,，、\n]/).map(s => s.trim()).filter(Boolean);

export function BookmarkNotes({ snapshot, onSnapshotChange, onOpenWorkDetail }: Props) {
  const [scope, setScope] = useState(snapshot.currentVersionId ? "current" : "all");
  const [query, setQuery] = useState("");
  const [search, setSearch] = useState("");
  const [offset, setOffset] = useState(0);
  const [revision, setRevision] = useState(0);
  const [page, setPage] = useState<BookmarkSearchPage>({ items: [], total: 0 });
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState("");
  const [tags, setTags] = useState("");
  const [editing, setEditing] = useState<BookmarkDto | null>(null);
  const [editNote, setEditNote] = useState("");
  const [editTags, setEditTags] = useState("");
  const [restore, setRestore] = useState<{ bundle: BookmarkBundle; report: BookmarkRestoreReport } | null>(null);
  const file = useRef<HTMLInputElement>(null);
  const sequence = useRef(0), lock = useRef(false);
  const id = useId();
  const versionId = scope === "current" ? snapshot.currentVersionId ?? "" : null;
  const workId = scope === "work" ? snapshot.currentWorkId ?? "" : null;
  const load = useCallback(async () => {
    const seq = ++sequence.current;
    setLoading(true); setError(""); setPage({items:[], total:0});
    try {
      const result = await callCommand<BookmarkSearchPage>("search_bookmarks", { workId, versionId, query: search, offset });
      if (seq === sequence.current) {
        if (offset > 0 && offset >= result.total) setOffset(Math.max(0, Math.floor((result.total - 1) / 100) * 100));
        else setPage(result);
      }
    } catch (e) { if (seq === sequence.current) setError(normalizeAppError(e).message); }
    finally { if (seq === sequence.current) setLoading(false); }
  }, [workId, versionId, search, offset]);
  useEffect(() => { const guard = sequence; void load(); return () => { ++guard.current; }; }, [load, revision]);
  useEffect(() => { setOffset(0); }, [versionId, workId]);

  async function run(action: () => Promise<void>) {
    if (lock.current) return;
    lock.current = true; setBusy(true); setError(""); setNotice("");
    try { await action(); } catch (e) { setError(normalizeAppError(e).message); }
    finally { lock.current = false; setBusy(false); }
  }
  async function exportNotes() {
    const targetPath = await save({title:"导出书签笔记", defaultPath:"asmr-bookmark-notes.json", filters:[{name:"书签备份",extensions:["json"]}]});
    if (!targetPath) return;
    const bundle = await callCommand<BookmarkBundle>("export_bookmark_notes", { targetPath });
    setNotice(`已导出全部 ${bundle.bookmarks.length} 个书签，包含笔记、标签及版本关联。`);
  }
  async function preview(file: File) {
    setRestore(null);
    if (file.size > 10 * 1024 * 1024) throw new Error("书签备份不能超过 10 MB");
    let bundle: BookmarkBundle;
    try { bundle = JSON.parse(await file.text()) as BookmarkBundle; }
    catch { throw new Error("无法读取书签备份，请选择有效的 JSON 文件"); }
    const report = await callCommand<BookmarkRestoreReport>("restore_bookmark_notes", { bundle, apply:false });
    setRestore({bundle,report});
  }
  return <section className="drawer-section bookmark-notes" aria-label="书签笔记" onKeyDown={e => e.stopPropagation()}>
    <h4><BookmarkCheck size={14}/><span className="ui-icon-label">书签笔记</span></h4>
    <div className="bookmark-notes-toolbar">
      <Select size="compact" aria-label="书签范围" value={scope} onValueChange={value => { setScope(value); setOffset(0); }}
        options={[{value:"current", label:"当前音轨", disabled:!snapshot.currentVersionId}, {value:"work", label:"当前作品", disabled:!snapshot.currentWorkId}, {value:"all", label:"全部作品"}]}/>
      <span className="text-muted">{loading ? "加载中…" : `${page.total} 个书签`}</span>
    </div>
    <form className="bookmark-notes-search" onSubmit={e => {e.preventDefault();setOffset(0);setSearch(query.trim());setRevision(value=>value+1);}}>
      <input className="input-text" aria-label="搜索书签笔记" placeholder="搜索笔记、标签、作品或音轨" maxLength={200} value={query} onChange={e=>setQuery(e.target.value)}/>
      <button className="btn-icon" type="submit" aria-label="搜索书签" title="搜索书签"><Search size={16}/></button>
    </form>
    <form className="bookmark-note-editor" aria-label="添加书签" onSubmit={e=>{e.preventDefault(); if (!snapshot.currentVersionId) return; void run(async()=>{
      await callCommand("add_bookmark",{versionId:snapshot.currentVersionId,positionMs:snapshot.positionMs,note:note.trim()||`书签 @ ${formatDurationMs(snapshot.positionMs)}`,tags:splitTags(tags)});
      setNote("");setTags("");setNotice("已保存到当前播放的音轨版本。");setRevision(value=>value+1);
    });}}>
      <label htmlFor={`${id}-note`} className="tabular-nums">当前播放位置 · {formatDurationMs(snapshot.positionMs)}</label>
      <textarea id={`${id}-note`} className="input-text" placeholder="记录这一刻的感受或内容…" rows={2} maxLength={10000} value={note} disabled={busy||!snapshot.currentVersionId} onChange={e=>setNote(e.target.value)}/>
      <label htmlFor={`${id}-tags`}>标签</label>
      <input id={`${id}-tags`} className="input-text" placeholder="用逗号分隔，最多 16 个" maxLength={1040} value={tags} disabled={busy||!snapshot.currentVersionId} onChange={e=>setTags(e.target.value)}/>
      <button type="submit" className="btn btn-primary btn-sm" disabled={busy||!snapshot.currentVersionId}><Plus size={14}/><span className="ui-icon-label">添加书签</span></button>
    </form>
    {error && <div role="alert" className="error-banner">{error}<button type="button" className="btn btn-ghost btn-sm" disabled={busy} onClick={()=>void load()}>重新加载</button></div>}
    {notice && <p role="status" className="text-muted">{notice}</p>}
    <ul className="bookmark-notes-list" aria-label="书签检索结果" aria-busy={loading}>
      {page.items.map(bm=><li key={bm.id} className="bookmark-note-card">
        <div className="bookmark-notes-toolbar"><span className="bookmark-time">{formatDurationMs(bm.positionMs)}</span><span className="text-muted">{bm.versionLabel || "原音轨版本"}</span></div>
        <p className="bookmark-note-location">{bm.workTitle || "原作品"} · {bm.trackTitle || "原音轨"}</p>
        {!bm.linked ? <p className="text-muted">未关联 · 未连接到原音轨版本，笔记已保留</p> : !bm.playable && <p className="text-muted">来源不可用 · 笔记已保留</p>}
        {editing?.id===bm.id ? <form className="bookmark-note-editor" onSubmit={e=>{e.preventDefault();void run(async()=>{
          await callCommand("update_bookmark",{bookmarkId:bm.id,note:editNote,tags:splitTags(editTags)}); setEditing(null); setRevision(value=>value+1);setNotice("书签笔记已更新。");
        });}}>
          <label htmlFor={`${id}-edit-note`}>编辑笔记</label><textarea autoFocus id={`${id}-edit-note`} className="input-text" maxLength={10000} rows={4} value={editNote} disabled={busy} onChange={e=>setEditNote(e.target.value)}/>
          <label htmlFor={`${id}-edit-tags`}>编辑标签</label><input id={`${id}-edit-tags`} className="input-text" maxLength={1040} value={editTags} disabled={busy} onChange={e=>setEditTags(e.target.value)}/>
          <div className="bookmark-notes-toolbar"><button className="btn btn-secondary btn-sm" disabled={busy}>保存笔记</button><button type="button" className="btn btn-ghost btn-sm" disabled={busy} onClick={()=>setEditing(null)}>取消编辑</button></div>
        </form> : <><p className="bookmark-note-text">{bm.note || "无笔记"}</p><div className="bookmark-note-tags">{(bm.tags??[]).map(tag=><span className="tag-pill" key={tag}><span className="ui-tag-label">{tag}</span></span>)}</div></>}
        <div className="bookmark-notes-toolbar">
          <button type="button" className="btn btn-secondary btn-sm" disabled={busy||!bm.linked||!bm.playable} onClick={()=>void run(async()=>onSnapshotChange(await callCommand<PlaybackSnapshot>("play_bookmark",{bookmarkId:bm.id})))}><Play size={13}/><span className="ui-icon-label">从书签播放</span></button>
          {onOpenWorkDetail && <button type="button" className="btn-icon" disabled={!bm.linked} title="定位所属音轨" aria-label="定位所属音轨" onClick={()=>onOpenWorkDetail(bm.workId,bm.versionId)}><FolderTree size={15}/></button>}
          <button type="button" className="btn-icon" disabled={busy} title="编辑书签" aria-label="编辑书签" onClick={()=>{setEditing(bm);setEditNote(bm.note);setEditTags((bm.tags??[]).join("，"));}}><Edit3 size={15}/></button>
          <button type="button" className="btn-icon" disabled={busy} title="删除书签" aria-label="删除书签" onClick={()=>void run(async()=>{await callCommand("delete_bookmark",{bookmarkId:bm.id});setRevision(value=>value+1);})}><Trash2 size={15}/></button>
        </div>
      </li>)}
    </ul>
    {!loading && page.total===0 && !error && <p className="empty-box">暂无匹配书签，可切换到全部作品或调整搜索词。</p>}
    {(page.total>100||offset>0) && <div className="bookmark-notes-toolbar"><button type="button" className="btn btn-ghost btn-sm" disabled={loading||offset===0} onClick={()=>setOffset(Math.max(0,offset-100))}>上一页</button><span>{Math.floor(offset/100)+1} / {Math.max(1,Math.ceil(page.total/100))}</span><button type="button" className="btn btn-ghost btn-sm" disabled={loading||offset+100>=page.total} onClick={()=>setOffset(offset+100)}>下一页</button></div>}
    <div className="bookmark-notes-toolbar">
      <button type="button" className="btn btn-ghost btn-sm" disabled={busy} onClick={()=>void run(exportNotes)}><Download size={14}/><span className="ui-icon-label">导出全部书签</span></button>
      <button type="button" className="btn btn-ghost btn-sm" disabled={busy} onClick={()=>file.current?.click()}><Upload size={14}/><span className="ui-icon-label">恢复书签</span></button>
      <input ref={file} type="file" accept=".json,application/json" hidden aria-label="书签备份文件" onChange={e=>{const selected=e.target.files?.[0];e.target.value="";if(selected) void run(()=>preview(selected));}}/>
    </div>
    {restore && <div className="bookmark-restore-preview" role="region" aria-label="书签恢复预览">
      <h4>恢复预览</h4><p>新增 {restore.report.restored} 个，其中 {restore.report.unlinked} 个保留为未关联笔记；跳过已有 {restore.report.skipped} 个。</p>
      <p className="text-muted">保留现有笔记。未找到原版本时不会改用同名音轨。恢复前自动备份当前书签。</p>
      <div className="bookmark-notes-toolbar"><button type="button" className="btn btn-secondary btn-sm" disabled={busy||restore.report.restored===0} onClick={()=>void run(async()=>{
        const report=await callCommand<BookmarkRestoreReport>("restore_bookmark_notes",{bundle:restore.bundle,apply:true});setRestore(null);setScope("all");setSearch("");setQuery("");setOffset(0);setRevision(value=>value+1);setNotice(`已恢复 ${report.restored} 个书签，${report.unlinked} 个未关联，跳过 ${report.skipped} 个已有书签。`);
      })}>恢复新增书签</button><button type="button" className="btn btn-ghost btn-sm" disabled={busy} onClick={()=>setRestore(null)}>取消恢复</button></div>
    </div>}
  </section>;
}
