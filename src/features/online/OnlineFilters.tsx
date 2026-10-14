import { X } from "lucide-react";
import { callCommand, OnlineFilter, ProviderTag } from "../../lib/api";
import { TagFilterPopover } from "../../components/TagFilterPopover";

const labels: Record<OnlineFilter["kind"], string> = { voiceActor: "声优", circle: "社团", tag: "标签" };
const loadOnlineTags = () => callCommand<ProviderTag[]>("list_online_tags");

export function OnlineFilters({ filters, onChange }: { filters: OnlineFilter[]; onChange: (filters: OnlineFilter[]) => void }) {
  return <TagFilterPopover value={filters.filter(filter => filter.kind === "tag").map(filter => filter.value)}
    loadTags={loadOnlineTags} countDescription="数量为站点作品总数。"
    onChange={tags => onChange([...filters.filter(filter => filter.kind !== "tag"), ...tags.map(value => ({ kind: "tag" as const, value, exclude: false }))])} />;
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
    {filters.map((filter, index) => chip(`${filter.exclude ? "排除" : "包含"}${labels[filter.kind]}：${filter.value}`,
      () => onChange(filters.filter((_, i) => i !== index)), `${filter.kind}-${filter.exclude}-${filter.value}`))}
    <button type="button" className="btn btn-ghost btn-sm" onClick={onReset}>重置筛选</button>
  </div>;
}
