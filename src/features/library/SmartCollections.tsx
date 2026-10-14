import { useCallback, useEffect, useRef, useState } from "react";
import { SlidersHorizontal } from "lucide-react";
import { Select } from "../../components/Select";
import { callCommand, CollectionFilters, normalizeAppError, SmartCollection } from "../../lib/api";

export const emptyCollectionFilters: CollectionFilters = {
  keyword: "", voiceActors: [], tags: [], collected: null, favorite: null, listening: "any", offline: null,
};
const terms = (value: string) => value.split(/[,，、\n]/).map(v => v.trim()).filter(Boolean);
const triValue = (value: boolean | null) => value === null ? "any" : String(value);
const triParse = (value: string) => value === "any" ? null : value === "true";

interface Props {
  filters: CollectionFilters | null;
  keyword: string;
  onChange: (filters: CollectionFilters | null) => void;
}

export function SmartCollections({ filters, keyword, onChange }: Props) {
  const [expanded, setExpanded] = useState(false);
  const [collections, setCollections] = useState<SmartCollection[]>([]);
  const [selectedId, setSelectedId] = useState("");
  const [name, setName] = useState("");
  const [voices, setVoices] = useState("");
  const [tags, setTags] = useState("");
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [status, setStatus] = useState("");
  const saving = useRef(false);
  const request = useRef(0);
  const selected = collections.find(c => c.id === selectedId);
  const current = { ...(filters ?? emptyCollectionFilters), keyword };
  const latestFilters = useRef(current);
  latestFilters.current = current;
  const dirty = !!selected && (name !== selected.name || JSON.stringify(current) !== JSON.stringify(selected.filters));

  const load = useCallback(async () => {
    const seq = ++request.current;
    setLoading(true);
    try {
      const list = await callCommand<SmartCollection[]>("list_smart_collections");
      if (seq === request.current) { setCollections(list ?? []); setError(""); }
    } catch (e) {
      if (seq === request.current) setError(normalizeAppError(e).message);
    } finally {
      if (seq === request.current) setLoading(false);
    }
  }, []);
  const invalidateLoad = useCallback(() => { request.current++; }, []);
  useEffect(() => { void load(); return invalidateLoad; }, [load, invalidateLoad]);

  function apply(collection: SmartCollection) {
    setSelectedId(collection.id); setName(collection.name);
    setVoices(collection.filters.voiceActors.join("，")); setTags(collection.filters.tags.join("，"));
    onChange(collection.filters);
  }
  function reset() {
    setSelectedId(""); setName(""); setVoices(""); setTags(""); setStatus(""); setError("");
    onChange(null);
  }
  function change(next: Partial<CollectionFilters>) { setStatus(""); onChange({ ...current, ...next }); }
  async function save(asNew: boolean) {
    if (saving.current || !name.trim()) return;
    saving.current = true; setBusy(true); setError(""); setStatus("");
    try {
      const saved = await callCommand<SmartCollection>("save_smart_collection", {
        id: asNew ? null : selectedId || null, name, filters: current,
      });
      setCollections(list => [...list.filter(c => c.id !== saved.id), saved].sort((a, b) => a.name.localeCompare(b.name)));
      // The shared search field can change while the save command is pending.
      if (JSON.stringify(latestFilters.current) === JSON.stringify(current)) apply(saved);
      else { setSelectedId(saved.id); setName(saved.name); }
      setStatus("集合已保存，作品变化后将自动更新结果。");
    } catch (e) { setError(normalizeAppError(e).message); }
    finally { saving.current = false; setBusy(false); }
  }
  async function remove() {
    if (saving.current || !selectedId) return;
    saving.current = true; setBusy(true); setError(""); setStatus("");
    try {
      await callCommand("remove_smart_collection", { id: selectedId });
      setCollections(list => list.filter(c => c.id !== selectedId)); setSelectedId(""); setName("");
      setStatus("集合已移除，当前条件保留为临时筛选。作品和文件不受影响。");
    } catch (e) { setError(normalizeAppError(e).message); }
    finally { saving.current = false; setBusy(false); }
  }

  return <section className="smart-collections" aria-label="智能集合">
    <div className="smart-collections-heading">
      <button className="btn btn-secondary btn-sm" type="button" aria-expanded={expanded} aria-controls="collection-editor" onClick={() => setExpanded(v => !v)}>
        <SlidersHorizontal size={15} />筛选与集合
      </button>
      <span className="text-secondary">{filters ? `${selected?.name ?? "临时筛选"}${dirty ? " · 未保存更改" : ""}` : "保存常用条件，让作品自动归类"}</span>
      {filters && <button className="btn btn-ghost btn-sm" type="button" disabled={busy} onClick={reset}>清除筛选</button>}
    </div>
    {expanded && <div id="collection-editor" className="smart-collections-editor">
      <fieldset disabled={busy}>
        <div className="smart-collections-fields">
          <label>已保存集合<Select aria-label="已保存集合" value={selectedId} disabled={loading || busy}
            options={[{ value: "", label: "临时筛选" }, ...collections.map(c => ({ value: c.id, label: c.name }))]}
            onValueChange={id => { const collection = collections.find(c => c.id === id); if (collection) { apply(collection); setStatus(""); } else { setSelectedId(""); setName(""); } }} /></label>
          <label>声优名称<input className="input-text" value={voices} maxLength={2048} placeholder="多位声优用逗号分隔" onChange={e => { setVoices(e.target.value); change({ voiceActors: terms(e.target.value) }); }} /></label>
          <label>标签名称<input className="input-text" value={tags} maxLength={2048} placeholder="多个标签用逗号分隔" onChange={e => { setTags(e.target.value); change({ tags: terms(e.target.value) }); }} /></label>
          <label>收藏状态<Select aria-label="收藏状态" value={triValue(current.collected)} disabled={busy} options={[{ value: "any", label: "不限收藏" }, { value: "true", label: "已收藏到音频库" }, { value: "false", label: "未收藏到音频库" }]} onValueChange={v => change({ collected: triParse(v) })} /></label>
          <label>喜欢状态<Select aria-label="喜欢状态" value={triValue(current.favorite)} disabled={busy} options={[{ value: "any", label: "不限喜欢" }, { value: "true", label: "已喜欢" }, { value: "false", label: "未喜欢" }]} onValueChange={v => change({ favorite: triParse(v) })} /></label>
          <label>收听状态<Select aria-label="收听状态" value={current.listening} disabled={busy} options={[{ value: "any", label: "不限收听" }, { value: "unplayed", label: "未听" }, { value: "inProgress", label: "在听" }, { value: "played", label: "听过" }]} onValueChange={v => change({ listening: v as CollectionFilters["listening"] })} /></label>
          <label>离线可用<Select aria-label="离线可用" value={triValue(current.offline)} disabled={busy} options={[{ value: "any", label: "不限来源" }, { value: "true", label: "有可用本地音轨" }, { value: "false", label: "无可用本地音轨" }]} onValueChange={v => change({ offline: triParse(v) })} /></label>
        </div>
        <p className="smart-collections-hint">关键词沿用上方搜索框。声优、标签按完整名称匹配，每类最多 16 项，所有条件同时满足。查询已入库记录（含未收藏作品）；在听表示已开始收听；听过表示已完整听完一个播放目录，或已手动标记。</p>
        <div className="smart-collections-save">
          <label>集合名称<input className="input-text" value={name} maxLength={64} placeholder="例如：未听的离线耳かき" onChange={e => setName(e.target.value)} /></label>
          <button type="button" className="btn btn-secondary btn-sm" disabled={busy || loading || !name.trim()} onClick={() => void save(false)}>{busy ? "正在保存…" : selectedId ? "保存更改" : "保存集合"}</button>
          {selectedId && <>
            <button type="button" className="btn btn-ghost btn-sm" disabled={busy || !name.trim() || name.trim().toLowerCase() === selected?.name.toLowerCase()} onClick={() => void save(true)}>另存为新集合</button>
            <button type="button" className="btn btn-ghost btn-sm" disabled={busy} onClick={() => void remove()}>移除集合</button>
          </>}
        </div>
        <p className="smart-collections-hint">修改名称或条件后点击“保存更改”。移除集合只移除规则，不移除作品或文件。</p>
      </fieldset>
    </div>}
    {status && <p className="smart-collections-message" role="status">{status}</p>}
    {error && <div className="smart-collections-message" role="alert">{error} <button type="button" className="btn btn-ghost btn-sm" disabled={busy || loading} onClick={() => void load()}>重新加载集合</button></div>}
  </section>;
}
