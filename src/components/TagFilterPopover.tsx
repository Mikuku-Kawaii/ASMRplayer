import { ErrorNotice } from "./ErrorNotice";
import { useEffect, useId, useLayoutEffect, useRef, useState } from "react";
import type { ReactNode, Dispatch, SetStateAction } from "react";
import { createPortal } from "react-dom";
import { SlidersHorizontal, X } from "lucide-react";
import { normalizeAppError } from "../lib/api";

export interface TagOption { id: string; name: string; label: string; count?: number; pinned?: number }
export const EXTRA_FILTER_TAGS = ["R18", "带字幕"];
const ordinaryCount = (values: string[]) => values.filter(value => !EXTRA_FILTER_TAGS.includes(value)).length;

export function TagFilterPopover({ value, onChange, loadTags, countDescription, cacheTags = true, placement = "below-end", extension }: {
  value: string[]; onChange: (value: string[]) => void; loadTags: () => Promise<TagOption[]>;
  countDescription: string; cacheTags?: boolean; placement?: "below-end" | "below-start";
  extension?: {
    label: string; activeCount: number; canClear: boolean;
    onOpen: () => void; onClear: () => void;
    render: (tags: ReactNode, draft: string[], setDraft: Dispatch<SetStateAction<string[]>>) => ReactNode;
  };
}) {
  const menuId = useId();
  const [open, setOpen] = useState(false);
  const [tags, setTags] = useState<TagOption[] | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const [retry, setRetry] = useState(0);
  const [draft, setDraft] = useState<string[]>([]);
  const [search, setSearch] = useState("");
  const [position, setPosition] = useState({ top: 0, left: 0, width: 560, maxHeight: 500 });
  const trigger = useRef<HTMLButtonElement>(null);
  const panel = useRef<HTMLDivElement>(null);
  const input = useRef<HTMLInputElement>(null);
  const count = value.length + (extension?.activeCount ?? 0);
  const close = () => { setOpen(false); trigger.current?.focus(); };

  useEffect(() => {
    if (!open || tags !== null) return;
    let active = true;
    setLoading(true); setError("");
    void loadTags().then(next => {
      if (active) { setTags(next); setLoading(false); }
    }).catch(e => { if (active) { setError(normalizeAppError(e).message); setLoading(false); } });
    return () => { active = false; };
  }, [open, retry, tags, loadTags]);

  useLayoutEffect(() => {
    if (!open) return;
    const place = () => {
      const rect = trigger.current?.getBoundingClientRect();
      if (!rect) return;
      const width = Math.min(560, window.innerWidth - 32);
      const below = window.innerHeight - rect.bottom - 24;
      const above = rect.top - 24;
      const useAbove = below < 280 && above > below;
      const maxHeight = Math.max(120, Math.min(560, useAbove ? above : below));
      setPosition({ width, left: Math.max(16, Math.min(placement === "below-start" ? rect.left : rect.right - width, window.innerWidth - width - 16)),
        top: useAbove ? rect.top - maxHeight - 8 : rect.bottom + 8, maxHeight });
    };
    place();
    const firstTab = panel.current?.querySelector<HTMLButtonElement>('[role="tab"]');
    (firstTab ?? input.current)?.focus({ preventScroll: !!firstTab });
    window.addEventListener("resize", place);
    window.addEventListener("scroll", place, true);
    const outside = (event: PointerEvent | FocusEvent) => {
      if (event.target instanceof Node && !panel.current?.contains(event.target) && !trigger.current?.contains(event.target)) setOpen(false);
    };
    document.addEventListener("pointerdown", outside);
    document.addEventListener("focusin", outside);
    return () => {
      window.removeEventListener("resize", place); window.removeEventListener("scroll", place, true);
      document.removeEventListener("pointerdown", outside); document.removeEventListener("focusin", outside);
    };
  }, [open, placement]);

  const sorted = [...(tags ?? [])].sort((a, b) => (a.pinned ?? 999) - (b.pinned ?? 999) || (b.count ?? 0) - (a.count ?? 0) || a.label.localeCompare(b.label, "zh-CN"));
  const popular = sorted.slice(0, 12);
  const remaining = sorted.slice(12).sort((a, b) => a.label.localeCompare(b.label, "zh-CN"));
  const matching = sorted.filter(tag => `${tag.label} ${tag.name}`.toLocaleLowerCase().includes(search.trim().toLocaleLowerCase()));
  const tagList = (items: TagOption[]) => <div className="online-tag-grid">{items.map(tag => {
    const checked = draft.includes(tag.name);
    return <button type="button" key={tag.id} className={`online-tag-option ${checked ? "is-selected" : ""}`} title={tag.label}
      aria-pressed={checked} disabled={!checked && !EXTRA_FILTER_TAGS.includes(tag.name) && ordinaryCount(draft) >= 5}
      onClick={() => setDraft(current => current.includes(tag.name) ? current.filter(name => name !== tag.name) : EXTRA_FILTER_TAGS.includes(tag.name) || ordinaryCount(current) < 5 ? [...current, tag.name] : current)}>
      <span className="online-tag-name">{tag.label}</span>
      {tag.count !== undefined && <span className="online-tag-count tabular-nums" aria-label={`${tag.count} 部作品`}>{tag.count.toLocaleString()}</span>}
    </button>;
  })}</div>;

  const tagContent = <>
      <input ref={input} className="input-text online-tag-search" aria-label="搜索标签" placeholder="搜索标签" value={search} onChange={event => setSearch(event.target.value)} />
      {draft.length > 0 && <div className="online-tag-selected">{draft.map(name => <button type="button" key={name} className="btn-chip active online-filter-chip"
        title={`取消选择${tags?.find(tag => tag.name === name)?.label ?? name}`} aria-label={`取消选择${tags?.find(tag => tag.name === name)?.label ?? name}`}
        onClick={() => setDraft(current => current.filter(value => value !== name))}><span>{tags?.find(tag => tag.name === name)?.label ?? name}</span><X size={12} /></button>)}</div>}
      <div className="online-tag-scroll" aria-busy={loading}>
        {loading && <p role="status" className="text-muted">正在获取标签…</p>}
        {error && <ErrorNotice message={error}><button type="button" className="btn btn-secondary" onClick={() => setRetry(value => value + 1)}>重试加载</button></ErrorNotice>}
        {tags !== null && (tags.length === 0 ? <p>暂无可用标签，可在作品标签更新后重新打开筛选。</p> : search.trim() ? <section aria-label="匹配标签"><h3>搜索结果</h3>{tagList(matching)}{!matching.length && <p>没有匹配的标签，试试其他名称。</p>}</section> : <>
          <section aria-label="热门标签"><h3>热门标签</h3>{tagList(popular)}</section>
          {remaining.length > 0 && <section aria-label="更多标签"><h3>更多标签</h3>{tagList(remaining)}</section>}
        </>)}
      </div>
      <p className="online-tag-hint text-muted">普通标签最多选 5 个，R18 和带字幕不占名额；同时满足所选条件。{countDescription}</p>
    </>;

  return <>
    <button ref={trigger} type="button" className={`btn-chip ${count || open ? "active" : ""}`}
      aria-expanded={open} aria-haspopup="dialog" aria-controls={open ? menuId : undefined}
      onKeyDown={event => event.stopPropagation()}
      onClick={() => {
        if (open) close();
        else { extension?.onOpen(); setDraft(value); if (!cacheTags) setTags(null); setSearch(""); setOpen(true); }
      }}><SlidersHorizontal size={13} aria-hidden="true" /><span className="ui-icon-label">筛选{count ? ` (${count})` : ""}</span></button>
    {open && createPortal(<div ref={panel} id={menuId} className="online-filter-popover" role="dialog"
      aria-label={extension?.label ?? "标签筛选"} style={position} onKeyDown={event => {
        event.stopPropagation();
        if (event.key === "Escape") { event.preventDefault(); close(); }
      }}>
      <div className="online-tag-heading"><strong>{extension?.label ?? "标签筛选"}</strong><span className="text-muted tabular-nums">已选 {ordinaryCount(draft)} / 5</span>
        <button className="btn-icon" type="button" aria-label={`关闭${extension?.label ?? "标签筛选"}`} title={`关闭${extension?.label ?? "标签筛选"}`} onClick={close}><X size={16} /></button></div>
      {extension ? extension.render(tagContent, draft, setDraft) : tagContent}
      <div className="online-tag-footer dialog-actions"><button type="button" className="btn btn-ghost" disabled={!draft.length && !extension?.canClear} onClick={() => { setDraft([]); extension?.onClear(); }}>清空选择</button>
        <button type="button" className="btn btn-primary" onClick={() => {
          onChange(draft); close();
        }}>应用筛选</button></div>
    </div>, document.body)}
  </>;
}
