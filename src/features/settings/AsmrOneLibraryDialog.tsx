import { useEffect, useMemo, useRef, useState } from "react";
import { ContentDialog } from "../../components/ContentDialog";
import { callCommand, normalizeAppError } from "../../lib/api";

interface Preview {
  id: string;
  favorites: number;
  playlists: number;
  playlistWorks: number;
  sitePlaylists: { id: string; name: string; workCount: number }[];
  alreadyInLibrary: number;
  works: { id: string; title: string; code: string }[];
}

type Choice = { id: string; title: string; detail: string };
type Selection = { workIds: Set<string>; playlistIds: Set<string> };
const pageSize = 40;

function GroupCheckbox({ title, name, count, total, summary, disabled, change }: {
  title: string; name: string; count: number; total: number; summary?: string; disabled: boolean; change: (checked: boolean) => void;
}) {
  return <label className="asmr-import-choice asmr-import-group-toggle">
    <input type="checkbox" aria-label={name} disabled={disabled} checked={count === total}
      ref={input => { if (input) input.indeterminate = count > 0 && count < total; }}
      onChange={event => change(event.target.checked)} />
    <span>{title}</span><small>{summary ?? `已选 ${count} / ${total}`}</small>
  </label>;
}

function SelectionGroup({ title, name, noun, choices, selected, disabled, change }: {
  title: string; name: string; noun: string; choices: Choice[]; selected: Set<string>; disabled: boolean;
  change: (checked: boolean, id?: string) => void;
}) {
  const [limit, setLimit] = useState(pageSize);
  if (!choices.length) return null;
  return <fieldset className="asmr-import-group">
    <legend><GroupCheckbox title={title} name={name} count={selected.size} total={choices.length} disabled={disabled} change={change} /></legend>
    <ul>{choices.slice(0, limit).map(choice => <li key={choice.id}>
      <label className="asmr-import-choice">
        <input type="checkbox" checked={selected.has(choice.id)} disabled={disabled} onChange={e => change(e.target.checked, choice.id)} />
        <span>{choice.title}</span><small>{choice.detail}</small>
      </label>
    </li>)}</ul>
    {limit < choices.length && <button type="button" className="btn btn-ghost btn-sm" onClick={() => setLimit(n => n + pageSize)}>显示更多{noun}</button>}
  </fieldset>;
}

export function AsmrOneLibraryDialog({ close }: { close: (added?: number, lists?: number) => void }) {
  const [preview, setPreview] = useState<Preview | null>(null);
  const [selection, setSelection] = useState<Selection>({ workIds: new Set(), playlistIds: new Set() });
  const [error, setError] = useState("");
  const [saving, setSaving] = useState(false);
  const request = useRef<Promise<Preview> | null>(null);
  const submitting = useRef(false);
  useEffect(() => {
    let live = true;
    request.current ??= callCommand<Preview>("preview_asmr_one_library");
    void request.current.then(result => {
      if (!live) return;
      setSelection({ workIds: new Set(result.works.map(w => w.id)), playlistIds: new Set(result.sitePlaylists.map(p => p.id)) });
      setPreview(result);
    }, failure => { if (live) setError(normalizeAppError(failure).message); });
    return () => { live = false; };
  }, []);
  const works = useMemo(() => preview?.works.map(w => ({ id: w.id, title: w.title, detail: w.code })) ?? [], [preview]);
  const lists = useMemo(() => preview?.sitePlaylists.map(p => ({ id: p.id, title: p.name, detail: `${p.workCount} 部` })) ?? [], [preview]);
  const total = works.length + lists.length;
  const selectedCount = selection.workIds.size + selection.playlistIds.size;

  function change(group: keyof Selection | "all", checked: boolean, id?: string) {
    setSelection(current => {
      function selected(key: keyof Selection, choices: Choice[]) {
        if (key !== group && group !== "all") return current[key];
        if (id === undefined) return new Set(checked ? choices.map(item => item.id) : []);
        const next = new Set(current[key]);
        if (checked) next.add(id); else next.delete(id);
        return next;
      }
      return { workIds: selected("workIds", works), playlistIds: selected("playlistIds", lists) };
    });
  }
  function reject() {
    if (submitting.current) return;
    void request.current?.then(p => callCommand("decide_asmr_one_library", { id: p.id, accept: false })).catch(() => {});
    close();
  }
  async function confirm() {
    if (!preview || !selectedCount || submitting.current) return;
    submitting.current = true;
    setSaving(true);
    setError("");
    try {
      const added = await callCommand<number>("decide_asmr_one_library", { id: preview.id, accept: true,
        selection: { workIds: [...selection.workIds], playlistIds: [...selection.playlistIds] } });
      close(added, selection.playlistIds.size);
    } catch (failure) {
      setError(normalizeAppError(failure).message);
    } finally { submitting.current = false; setSaving(false); }
  }
  return <ContentDialog label="同步收藏与播放列表" onClose={reject} busy={saving}>
    <div className="modal-header"><h2>同步收藏与播放列表</h2></div>
    <div className="dialog-body">
      {error && <p role="alert" className="asmr-one-login-error">{error}</p>}
      {!preview && !error && <p role="status">正在读取 asmr.one 的收藏与播放列表…</p>}
      {preview && <>
        <p>收藏 {preview.favorites} 部，{preview.playlists} 个播放列表共含 {preview.playlistWorks} 部。合并重复作品后，已有 {preview.alreadyInLibrary} 部在音频库。</p>
        {lists.length > 0 && <p className="asmr-import-hint">勾选的歌单将保存为作品集，收录已在库和本次勾选的作品。</p>}
        {works.length > 0 && lists.length > 0 && <GroupCheckbox title="全选" name="全选" count={selectedCount} total={total}
          summary={`${selection.workIds.size} 部作品 · ${selection.playlistIds.size} 个歌单`} disabled={saving} change={checked => change("all", checked)} />}
        <SelectionGroup title="保留官网歌单" name="全选歌单" noun="歌单" choices={lists} selected={selection.playlistIds} disabled={saving}
          change={(checked, id) => change("playlistIds", checked, id)} />
        <SelectionGroup title="加入音频库的作品" name="全选作品" noun="作品" choices={works} selected={selection.workIds} disabled={saving}
          change={(checked, id) => change("workIds", checked, id)} />
        {!works.length && <p role="status">没有待加入的作品。</p>}
      </>}
    </div>
    <div className="modal-footer">
      <button type="button" className="btn btn-secondary" disabled={saving} autoFocus onClick={reject}>
        {(preview && !total) || (!preview && error) ? "关闭" : "拒绝"}</button>
      {total > 0 && <button type="button" className="btn btn-primary" disabled={saving || !selectedCount} onClick={() => void confirm()}>
        {saving ? "正在保存…" : selection.workIds.size ? `确认加入 ${selection.workIds.size} 部` : selection.playlistIds.size ? `保存 ${selection.playlistIds.size} 个作品集` : "确认加入"}</button>}
    </div>
  </ContentDialog>;
}
