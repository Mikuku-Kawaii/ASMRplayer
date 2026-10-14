import { X } from "lucide-react";
import { useCallback } from "react";
import { TagFilterPopover, TagOption } from "../../components/TagFilterPopover";
import { callCommand, WorkSummary } from "../../lib/api";

const normalize = (tag: string) => tag.trim().toLocaleLowerCase();
async function loadLibraryTags(scope: string): Promise<TagOption[]> {
  // Count the entire library, independently of keyword, active tags and display pagination.
  const works = await callCommand<WorkSummary[]>("list_works", { filter: scope, search: null });
  const tags = new Map<string, TagOption>();
  for (const work of works) {
    const seen = new Set<string>();
    for (const raw of work.tags) {
      const key = normalize(raw);
      if (!key || seen.has(key)) continue;
      seen.add(key);
      const existing = tags.get(key);
      if (existing) existing.count++;
      else tags.set(key, { id: key, name: raw.trim(), label: raw.trim(), count: 1 });
    }
  }
  return [...tags.values()];
}

export function matchesLibraryTags(work: WorkSummary, selected: string[]) {
  const actual = new Set(work.tags.map(normalize));
  return selected.every(tag => actual.has(normalize(tag)));
}

export function LibraryFilters({ value, onChange, scope = "all" }: { value: string[]; onChange: (value: string[]) => void; scope?: "all" | "local" | "favorites" | "recent" }) {
  const loadTags = useCallback(() => loadLibraryTags(scope), [scope]);
  return <TagFilterPopover value={value} onChange={onChange} loadTags={loadTags} cacheTags={false}
    countDescription={scope === "favorites" ? "数量为喜欢列表内作品总数。" : scope === "recent" ? "数量为历史收听列表内作品总数。" : "数量为我的音频库内作品总数。"} />;
}

export function LibraryFilterSummary({ value, onChange }: { value: string[]; onChange: (value: string[]) => void }) {
  if (!value.length) return null;
  return <div className="online-filter-summary" aria-label="生效筛选条件">
    {value.map(tag => <button key={tag} type="button" className="btn-chip active online-filter-chip"
      aria-label={`移除标签：${tag}`} title={`移除标签：${tag}`} onClick={() => onChange(value.filter(item => item !== tag))}>
      <span>标签：{tag}</span><X size={13} aria-hidden="true" />
    </button>)}
    <button type="button" className="btn btn-ghost btn-sm" onClick={() => onChange([])}>重置筛选</button>
  </div>;
}
