import { X } from "lucide-react";
import { callCommand, OnlineFilter, ProviderTag } from "../../lib/api";
import { TagFilterPopover, TagOption } from "../../components/TagFilterPopover";

const labels: Record<OnlineFilter["kind"], string> = { voiceActor: "声优", circle: "社团", tag: "标签", age: "分级" };
const loadOnlineTags = async (): Promise<TagOption[]> => [
  { id: "rating-r18", name: "R18", label: "R18", pinned: 1 },
  { id: "has-subtitle", name: "带字幕", label: "带字幕", pinned: 2 },
  ...(await callCommand<ProviderTag[]>("list_online_tags")).filter(tag => !["r18", "带字幕"].includes(tag.name.trim().toLowerCase())),
];

export function OnlineFilters({ filters, onChange, subtitleOnly, onSubtitleChange }: {
  filters: OnlineFilter[]; onChange: (filters: OnlineFilter[]) => void; subtitleOnly: boolean; onSubtitleChange: (value: boolean) => void;
}) {
  return <TagFilterPopover value={[...filters.filter(filter => filter.kind === "tag" || filter.kind === "age").map(filter => filter.kind === "age" ? "R18" : filter.value), ...(subtitleOnly ? ["带字幕"] : [])]}
    loadTags={loadOnlineTags} countDescription="数量为站点作品总数。"
    onChange={tags => {
      onSubtitleChange(tags.includes("带字幕"));
      onChange([...filters.filter(filter => filter.kind !== "tag" && filter.kind !== "age"), ...tags.filter(tag => tag !== "带字幕").map((value): OnlineFilter => value === "R18"
        ? { kind: "age", value: "adult", exclude: false } : { kind: "tag", value, exclude: false })]);
    }} />;
}

export function OnlineFilterSummary({ filters, onChange, keyword, subtitleOnly, onClearKeyword, onClearSubtitle, onReset }: {
  filters: OnlineFilter[]; onChange: (filters: OnlineFilter[]) => void; keyword: string; subtitleOnly: boolean;
  onClearKeyword: () => void; onClearSubtitle: () => void; onReset: () => void;
}) {
  if (!filters.length && !keyword && !subtitleOnly) return null;
  const chip = (label: string, remove: () => void, key: string) => <button key={key} type="button" className="btn-chip active online-filter-chip"
    onClick={remove} aria-label={`移除${label}`} title={`移除${label}`}><span>{label}</span><X size={13} aria-hidden="true" /></button>;
  return <div className="online-filter-summary" aria-label="生效筛选条件">
    {keyword && chip(`关键词：${keyword}`, onClearKeyword, "keyword")}
    {subtitleOnly && chip("仅带字幕", onClearSubtitle, "subtitle")}
    {filters.map((filter, index) => chip(`${filter.exclude ? "排除" : "包含"}${labels[filter.kind]}：${filter.kind === "age" ? "R18" : filter.value}`,
      () => onChange(filters.filter((_, i) => i !== index)), `${filter.kind}-${filter.exclude}-${filter.value}`))}
    <button type="button" className="btn btn-ghost btn-sm" onClick={onReset}>重置筛选</button>
  </div>;
}
