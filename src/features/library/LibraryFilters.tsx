import { X } from "lucide-react";
import { useCallback, useId, useState } from "react";
import { EXTRA_FILTER_TAGS, TagFilterPopover, TagOption } from "../../components/TagFilterPopover";
import { callCommand, CollectionFilters, WorkSummary } from "../../lib/api";
import { CollectionFilterFields, LibraryCollectionState, libraryCollectionDefaults, SmartCollections } from "./SmartCollections";

const normalize = (tag: string) => tag.trim().toLocaleLowerCase();
async function loadLibraryTags(scope: string): Promise<{ tags: TagOption[]; voices: string[] }> {
  // Count the entire library, independently of keyword, active tags and display pagination.
  const works = await callCommand<WorkSummary[]>("list_works", { filter: scope, search: null });
  const tags = new Map<string, TagOption>();
  for (const work of works) {
    const seen = new Set<string>();
    for (const raw of work.tags) {
      const key = normalize(raw);
      if (!key || key === "r18" || key === "带字幕" || seen.has(key)) continue;
      seen.add(key);
      const existing = tags.get(key);
      if (existing) existing.count = (existing.count ?? 0) + 1;
      else tags.set(key, { id: key, name: raw.trim(), label: raw.trim(), count: 1 });
    }
  }
  return { voices: [...new Map(works.flatMap(work => work.voiceActors).map(name => [normalize(name), name.trim()])).values()].filter(Boolean).sort((a, b) => a.localeCompare(b, "zh-CN")), tags: [
    { id: "rating-r18", name: "R18", label: "R18", count: works.filter(work => work.nsfw === true).length, pinned: 1 },
    { id: "has-subtitle", name: "带字幕", label: "带字幕", count: works.filter(work => work.hasSubtitle === true).length, pinned: 2 },
    ...tags.values(),
  ] };
}

export function addLibraryTag(selected: string[], raw: string): string[] {
  const tag = raw.trim();
  if (!tag || selected.some(value => normalize(value) === normalize(tag))) return selected;
  const extra = EXTRA_FILTER_TAGS.find(value => normalize(value) === normalize(tag));
  if (!extra && selected.filter(value => !EXTRA_FILTER_TAGS.some(extra => normalize(extra) === normalize(value))).length >= 5) return selected;
  return [...selected, extra ?? tag];
}

export function matchesLibraryTags(work: WorkSummary, selected: string[]) {
  const actual = new Set(work.tags.map(normalize));
  return selected.every(tag => normalize(tag) === "r18" ? work.nsfw === true : tag === "带字幕" ? work.hasSubtitle === true : actual.has(normalize(tag)));
}

const conditions = (filters: CollectionFilters | null): { id: string; label: string; reset: Partial<CollectionFilters> }[] => filters ? [
  ...filters.voiceActors.map(name => ({ id: `voice:${name}`, label: `声优：${name}`, reset: { voiceActors: filters.voiceActors.filter(voice => voice !== name) } })),
  { id: "collected", label: filters.collected === true ? "" : filters.collected === false ? "未收藏" : "不限收藏", reset: { collected: true } },
  { id: "favorite", label: filters.favorite === null ? "" : filters.favorite ? "已喜欢" : "未喜欢", reset: { favorite: null } },
  { id: "listening", label: { any: "", played: "听过", inProgress: "在听", unplayed: "未听" }[filters.listening], reset: { listening: "any" as const } },
  { id: "offline", label: filters.offline === null ? "" : filters.offline ? "离线可用" : "无可用本地音轨", reset: { offline: null } },
].filter(condition => condition.label) : [];

export function LibraryFilters({ value, onChange, scope = "all", collectionState, keyword = "", onCollectionChange }: {
  value: string[]; onChange: (value: string[]) => void; scope?: "all" | "local" | "favorites" | "recent" | "collection";
  collectionState?: LibraryCollectionState; keyword?: string;
  onCollectionChange?: (filters: CollectionFilters | null, selectedId: string) => void;
}) {
  const id = useId();
  const [page, setPage] = useState("filters");
  const [collectionsVisited, setCollectionsVisited] = useState(false);
  const [draft, setDraft] = useState<LibraryCollectionState>({ filters: null, selectedId: "" });
  const [draftKeyword, setDraftKeyword] = useState(keyword);
  const [voices, setVoices] = useState<string[]>([]);
  const loadTags = useCallback(async () => { const options = await loadLibraryTags(scope); setVoices(options.voices); return options.tags; }, [scope]);
  return <TagFilterPopover value={value} onChange={tags => {
    onChange(tags);
    if (onCollectionChange) {
      const filters = draft.filters || draftKeyword !== keyword ? { ...(draft.filters ?? libraryCollectionDefaults), keyword: draftKeyword, tags } : null;
      onCollectionChange(filters, draft.selectedId);
    }
  }} loadTags={loadTags} cacheTags={false} placement="below-start"
    extension={onCollectionChange ? {
      label: "音频库筛选", activeCount: conditions(collectionState?.filters ?? null).length,
      canClear: draft.filters !== null || !!draft.selectedId,
      onOpen: () => { setPage("filters"); setCollectionsVisited(false); setDraft(collectionState ?? { filters: null, selectedId: "" }); setDraftKeyword(keyword); },
      onClear: () => { setDraft({ filters: null, selectedId: "" }); setDraftKeyword(keyword); },
      render: (tags, draftTags, setDraftTags) => <>
        <div className="library-filter-tabs" role="tablist" aria-label="筛选页面">
          {[{ key: "filters", label: "筛选" }, { key: "collections", label: "集合" }].map((tab, index) => <button type="button" key={tab.key}
            role="tab" id={`${id}-${tab.key}-tab`} aria-selected={page === tab.key} aria-controls={`${id}-${tab.key}-panel`}
            tabIndex={page === tab.key ? 0 : -1} className={`btn btn-ghost ${page === tab.key ? "active" : ""}`}
            onClick={() => { setPage(tab.key); if (tab.key === "collections") setCollectionsVisited(true); }} onKeyDown={event => {
              if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) return;
              event.preventDefault();
              const next = event.key === "Home" ? "filters" : event.key === "End" ? "collections" : index === 0 ? "collections" : "filters";
              setPage(next); if (next === "collections") setCollectionsVisited(true); document.getElementById(`${id}-${next}-tab`)?.focus();
            }}>{tab.label}</button>)}
        </div>
        <div id={`${id}-filters-panel`} role="tabpanel" aria-labelledby={`${id}-filters-tab`} hidden={page !== "filters"} className="library-filter-page">
          <div className="library-filter-fields">
            <CollectionFilterFields value={draft.filters ?? libraryCollectionDefaults} voiceOptions={voices} onChange={filters => setDraft(previous => ({ ...previous, filters }))} />
            <p className="smart-collections-hint">听过：已完整听完一个播放目录，或已手动标记；在听：已开始但未听完；未听：尚未开始。</p>
          </div>
          {tags}
        </div>
        <div id={`${id}-collections-panel`} role="tabpanel" aria-labelledby={`${id}-collections-tab`} hidden={page !== "collections"} className="library-filter-page">
          {collectionsVisited && <SmartCollections managementOnly filters={draft.filters} keyword={draftKeyword} sharedTags={draftTags}
            selectedId={draft.selectedId} defaults={libraryCollectionDefaults} onChange={(filters, selectedId) => {
              setDraft({ filters, selectedId }); setDraftKeyword(filters?.keyword ?? ""); setDraftTags(filters?.tags ?? []);
            }} />}
        </div>
      </>,
    } : undefined}
    countDescription={scope === "collection" ? "数量为已入库记录总数，含未收藏作品。" : scope === "favorites" ? "数量为喜欢列表内作品总数。" : scope === "recent" ? "数量为历史收听列表内作品总数。" : "数量为我的音频库内作品总数。"} />;
}

export function LibraryFilterSummary({ value, onChange, resetLabel = "重置筛选", filters = null, onReset, onFiltersChange }: { value: string[]; onChange: (value: string[]) => void; resetLabel?: string; filters?: CollectionFilters | null; onReset?: () => void; onFiltersChange?: (filters: CollectionFilters) => void }) {
  const activeConditions = conditions(filters);
  if (!value.length && !activeConditions.length) return null;
  return <div className="online-filter-summary" aria-label="生效筛选条件">
    {value.map(tag => <button key={tag} type="button" className="btn-chip active online-filter-chip"
      aria-label={`移除标签：${tag}`} title={`移除标签：${tag}`} onClick={() => onChange(value.filter(item => item !== tag))}>
      <span>标签：{tag}</span><X size={13} aria-hidden="true" />
    </button>)}
    {activeConditions.map(({ id, label, reset }) => <button type="button" className="btn-chip active online-filter-chip" key={id}
      title={`移除${label}`} aria-label={`移除${label}`} onClick={() => { if (filters) onFiltersChange?.({ ...filters, ...reset }); }}><span>{label}</span><X size={13} aria-hidden="true" /></button>)}
    <button type="button" className="btn btn-ghost btn-sm" onClick={() => { onChange([]); onReset?.(); }}>{resetLabel}</button>
  </div>;
}
