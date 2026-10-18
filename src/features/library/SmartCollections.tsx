import { ErrorNotice } from "../../components/ErrorNotice";
import { useCallback, useEffect, useRef, useState } from "react";
import { SlidersHorizontal } from "lucide-react";
import { Select } from "../../components/Select";
import { callCommand, CollectionFilters, normalizeAppError, SmartCollection } from "../../lib/api";

export const emptyCollectionFilters: CollectionFilters = {
  keyword: "", voiceActors: [], tags: [], collected: null, favorite: null, listening: "any", offline: null,
};
export interface LibraryCollectionState { filters: CollectionFilters | null; selectedId: string }
export const libraryCollectionDefaults: CollectionFilters = { ...emptyCollectionFilters, collected: true };
const terms = (value: string) => value.split(/[,，、\n]/).map(v => v.trim()).filter(Boolean);
function FilterTagGroup<T extends string | boolean | null>({ label, value, options, onChange }: {
  label: string; value: T; options: { value: T; label: string }[]; onChange: (value: T) => void;
}) {
  return <fieldset className="library-filter-tag-group"><legend>{label}</legend><div className="online-tag-grid">
    {options.map(option => <button type="button" key={String(option.value)} className={`online-tag-option ${value === option.value ? "is-selected" : ""}`}
      aria-pressed={value === option.value} onClick={() => onChange(option.value)}>{option.label}</button>)}
  </div></fieldset>;
}

export function CollectionFilterFields({ value, onChange, voiceOptions = [] }: { value: CollectionFilters; onChange: (next: CollectionFilters) => void; voiceOptions?: string[] }) {
  const [voiceSearch, setVoiceSearch] = useState("");
  const change = (next: Partial<CollectionFilters>) => onChange({ ...value, ...next });
  const normalizeVoice = (name: string) => name.trim().toLocaleLowerCase();
  const selected = (name: string) => value.voiceActors.some(voice => normalizeVoice(voice) === normalizeVoice(name));
  const addVoice = (name: string) => {
    const trimmed = name.trim();
    if (trimmed && !selected(trimmed) && value.voiceActors.length < 16) change({ voiceActors: [...value.voiceActors, trimmed] });
  };
  const availableVoices = voiceOptions.filter(name => !selected(name) && normalizeVoice(name).includes(normalizeVoice(voiceSearch)));
  return <div className="library-filter-tag-fields">
    <FilterTagGroup label="收听状态" value={value.listening} options={[{ value: "any", label: "不限收听" }, { value: "played", label: "听过" }, { value: "inProgress", label: "在听" }, { value: "unplayed", label: "未听" }]} onChange={listening => change({ listening })} />
    <FilterTagGroup label="喜欢状态" value={value.favorite} options={[{ value: null, label: "不限喜欢" }, { value: true, label: "已喜欢" }, { value: false, label: "未喜欢" }]} onChange={favorite => change({ favorite })} />
    <FilterTagGroup label="离线可用" value={value.offline} options={[{ value: null, label: "不限来源" }, { value: true, label: "离线可用" }, { value: false, label: "无本地音轨" }]} onChange={offline => change({ offline })} />
    <fieldset className="library-filter-tag-group"><legend>声优</legend>
      <input className="input-text" aria-label="搜索声优" value={voiceSearch} maxLength={128} placeholder="搜索声优，按 Enter 添加" onChange={event => setVoiceSearch(event.target.value)} onKeyDown={event => {
        if (event.key === "Enter" && !event.nativeEvent.isComposing) { event.preventDefault(); event.stopPropagation(); addVoice(voiceSearch); setVoiceSearch(""); }
      }} />
      <div className="online-tag-grid library-voice-tags">
        <button type="button" className={`online-tag-option ${!value.voiceActors.length ? "is-selected" : ""}`} aria-pressed={!value.voiceActors.length} onClick={() => change({ voiceActors: [] })}>不限声优</button>
        {value.voiceActors.map(name => <button type="button" key={name} className="online-tag-option is-selected" aria-pressed="true" title={`移除声优：${name}`} onClick={() => change({ voiceActors: value.voiceActors.filter(voice => voice !== name) })}>{name}</button>)}
        {availableVoices.map(name => <button type="button" key={name} className="online-tag-option" aria-pressed="false" disabled={value.voiceActors.length >= 16} onClick={() => addVoice(name)}>{name}</button>)}
      </div>
    </fieldset>
  </div>;
}

interface Props {
  filters: CollectionFilters | null;
  keyword: string;
  sharedTags?: string[];
  selectedId?: string;
  defaults?: CollectionFilters;
  managementOnly?: boolean;
  onChange: (filters: CollectionFilters | null, selectedId: string) => void;
}

export function SmartCollections({ filters, keyword, sharedTags, selectedId: controlledId, defaults = emptyCollectionFilters, managementOnly = false, onChange }: Props) {
  const [expanded, setExpanded] = useState(false);
  const [collections, setCollections] = useState<SmartCollection[]>([]);
  const [localId, setSelectedId] = useState("");
  const selectedId = controlledId ?? localId;
  const [name, setName] = useState("");
  const [tags, setTags] = useState(() => filters?.tags.join("，") ?? "");
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [status, setStatus] = useState("");
  const saving = useRef(false);
  const request = useRef(0);
  const selected = collections.find(c => c.id === selectedId);
  const current = { ...(filters ?? defaults), keyword, ...(sharedTags ? { tags: sharedTags } : {}) };
  const latestFilters = useRef(current);
  latestFilters.current = current;
  const dirty = !!selected && (name !== selected.name || JSON.stringify(current) !== JSON.stringify(selected.filters));
  useEffect(() => { setName(selected?.name ?? ""); }, [selected?.id, selected?.name]);
  const conditions = [
    ...(managementOnly ? [current.keyword ? `关键词：${current.keyword}` : "", ...current.tags.map(tag => `标签：${tag}`)] : []),
    ...current.voiceActors,
    current.collected === null ? "不限收藏" : current.collected ? "已收藏" : "未收藏",
    current.favorite === null ? "" : current.favorite ? "已喜欢" : "未喜欢",
    { any: "", unplayed: "未听", inProgress: "在听", played: "听过" }[current.listening],
    current.offline === null ? "" : current.offline ? "离线可用" : "无可用本地音轨",
  ].filter(Boolean).join(" · ");

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
    setTags(collection.filters.tags.join("，"));
    onChange(collection.filters, collection.id);
  }
  function reset() {
    setSelectedId(""); setName(""); setTags(""); setStatus(""); setError("");
    onChange(null, "");
  }
  function change(next: Partial<CollectionFilters>) { setStatus(""); onChange({ ...current, ...next }, selectedId); }
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
      else { setSelectedId(saved.id); setName(saved.name); onChange(latestFilters.current, saved.id); }
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
      onChange(latestFilters.current, "");
      setStatus("集合已移除，当前条件保留为临时筛选。作品和文件不受影响。");
    } catch (e) { setError(normalizeAppError(e).message); }
    finally { saving.current = false; setBusy(false); }
  }

  return <section className={`smart-collections${managementOnly ? " smart-collections-embedded" : ""}`} aria-label="智能集合">
    <div className="smart-collections-heading">
      {!managementOnly && <button className="btn btn-secondary btn-sm" type="button" aria-expanded={expanded} aria-controls="collection-editor" onClick={() => setExpanded(v => !v)}>
        <SlidersHorizontal size={15} /><span className="ui-icon-label">筛选与集合
      </span></button>}
      <span className="text-secondary">{filters ? `${selected?.name ?? "临时筛选"}${dirty ? " · 未保存更改" : ""} · ${conditions}` : "保存常用条件，让作品自动归类"}</span>
      {filters && !managementOnly && <button className="btn btn-ghost btn-sm" type="button" disabled={busy} onClick={reset}>清除筛选</button>}
    </div>
    {(expanded || managementOnly) && <div id="collection-editor" className="smart-collections-editor">
      <fieldset disabled={busy}>
        <div className="smart-collections-fields">
          <label>已保存集合<Select aria-label="已保存集合" value={selectedId} disabled={loading || busy}
            options={[{ value: "", label: "临时筛选" }, ...collections.map(c => ({ value: c.id, label: c.name }))]}
            onValueChange={id => { const collection = collections.find(c => c.id === id); if (collection) { apply(collection); setStatus(""); } else { setSelectedId(""); setName(""); onChange(current, ""); } }} /></label>
          {!managementOnly && <><CollectionFilterFields value={current} onChange={change} />
          {!sharedTags && <label>标签名称<input className="input-text" value={tags} maxLength={2048} placeholder="多个标签用逗号分隔" onChange={e => { setTags(e.target.value); change({ tags: terms(e.target.value) }); }} /></label>}
          </>}
        </div>
        {!managementOnly && <p className="smart-collections-hint">{sharedTags ? "关键词和标签沿用上方搜索与筛选。" : "关键词沿用上方搜索框。"}声优、普通标签按完整名称匹配，所有条件同时满足。选择“不限收藏”可查询已入库的未收藏作品；在听表示已开始收听；听过表示已完整听完一个播放目录，或已手动标记。</p>}
        <div className="smart-collections-save">
          <label>集合名称<input className="input-text" value={name} maxLength={64} placeholder="例如：未听的离线耳かき" onChange={e => setName(e.target.value)} /></label>
          <button type="button" className="btn btn-secondary btn-sm" disabled={busy || loading || !name.trim()} onClick={() => void save(false)}>{busy ? "正在保存…" : selectedId ? "保存更改" : "保存集合"}</button>
          {selectedId && <>
            <button type="button" className="btn btn-ghost btn-sm" disabled={busy || !name.trim() || name.trim().toLowerCase() === selected?.name.toLowerCase()} onClick={() => void save(true)}>另存为新集合</button>
            <button type="button" className="btn btn-ghost btn-sm" disabled={busy} onClick={() => void remove()}>移除集合</button>
          </>}
        </div>
        <p className="smart-collections-hint">{managementOnly ? "选择集合后点击“应用筛选”。保存或移除会立即更新集合记录，作品和文件不受影响。" : "修改名称或条件后点击“保存更改”。移除集合只移除规则，不移除作品或文件。"}</p>
      </fieldset>
    </div>}
    {status && <p className="smart-collections-message" role="status">{status}</p>}
    {error && <ErrorNotice message={error}><button type="button" className="btn btn-ghost btn-sm" disabled={busy || loading} onClick={() => void load()}>重新加载集合</button></ErrorNotice>}
  </section>;
}
