import { useLayoutEffect, useRef, useState, type CSSProperties } from "react";
import { X } from "lucide-react";
import { ContentDialog } from "./ContentDialog";
import { fitCardTags } from "./fitCardTags";

/** Two measured rows, with the remaining tags available through the final pill. */
export function WorkCardTags({ tags, onTag }: { tags: string[]; onTag: (tag: string) => void }) {
  const root = useRef<HTMLDivElement>(null);
  const measures = useRef<HTMLDivElement>(null);
  const [layout, setLayout] = useState<{ count: number; tagWidth: number } | null>(null);
  const [open, setOpen] = useState(false);
  useLayoutEffect(() => {
    const element = root.current, probes = measures.current;
    if (!element || !probes) return;
    const measure = () => {
      const bounds = element.getBoundingClientRect();
      if (!bounds.width) return;
      // Normalize CSS zoom and the card's entrance transform before packing rows.
      const width = parseFloat(getComputedStyle(element).width) || bounds.width;
      const scale = bounds.width / width;
      const widths = (selector: string) => Array.from(probes.querySelectorAll<HTMLElement>(selector), node => node.getBoundingClientRect().width / scale);
      const tagsWidth = widths("[data-tag-measure]"), countsWidth = widths("[data-count-measure]");
      const gap = parseFloat(getComputedStyle(element).columnGap) || 0;
      let count = fitCardTags(tagsWidth, countsWidth, width, gap), tagWidth = width;
      if (count < tags.length) {
        // A very long pill must leave room for +N instead of emptying the second row.
        tagWidth = Math.max(1, width - Math.max(...countsWidth) - gap);
        count = fitCardTags(tagsWidth.map(value => Math.min(value, tagWidth)), countsWidth, width, gap);
      }
      setLayout(previous => previous?.count === count && previous.tagWidth === tagWidth ? previous : { count, tagWidth });
    };
    measure();
    const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(measure);
    observer?.observe(element);
    // Font family/weight changes can alter pill widths without changing the card width.
    for (const node of probes.children) observer?.observe(node);
    let disposed = false;
    void document.fonts?.ready.then(() => { if (!disposed) measure(); });
    window.addEventListener("resize", measure);
    return () => { disposed = true; observer?.disconnect(); window.removeEventListener("resize", measure); };
  }, [tags]);
  if (!tags.length) return null;
  const visible = Math.min(layout?.count ?? tags.length, tags.length);
  const hidden = tags.length - visible;
  return <div ref={root} className="work-tags work-card-tags" aria-label="作品标签" onClick={event => event.stopPropagation()}
    style={layout ? { "--work-card-tag-max-width": `${layout.tagWidth}px` } as CSSProperties : undefined}>
    <div ref={measures} className="work-card-tag-measures" aria-hidden="true">
      {tags.map((tag, index) => <span key={`tag-${index}`} data-tag-measure className="tag-pill tag-pill-btn"><span className="ui-tag-label">{tag}</span></span>)}
      {tags.map((_, index) => <span key={`count-${index}`} data-count-measure className="tag-pill tag-pill-btn"><span className="ui-tag-label">+{index + 1}</span></span>)}
    </div>
    {tags.slice(0, visible).map(tag => <button key={tag} type="button" className="tag-pill tag-pill-btn" title={`筛选标签: ${tag}`} onClick={() => onTag(tag)}>
      <span className="ui-tag-label">{tag}</span>
    </button>)}
    {hidden > 0 && <button type="button" className="tag-pill tag-pill-btn" aria-label={`+${hidden}，查看其余标签`}
      title={tags.slice(visible).join("、")} aria-haspopup="dialog" aria-expanded={open} onClick={() => setOpen(true)}>
      <span className="ui-tag-label">+{hidden}</span>
    </button>}
    {open && <ContentDialog label="全部作品标签" onClose={() => setOpen(false)}>
      <header className="modal-header"><h3>全部作品标签</h3><button type="button" className="btn-icon" aria-label="关闭标签" title="关闭标签" onClick={() => setOpen(false)}><X size={18} aria-hidden="true" /></button></header>
      <div className="work-card-tag-dialog-list">{tags.map(tag => <button key={tag} type="button" className="tag-pill tag-pill-btn" title={`筛选标签: ${tag}`}
        onClick={() => { setOpen(false); onTag(tag); }}><span className="ui-tag-label">{tag}</span></button>)}</div>
    </ContentDialog>}
  </div>;
}
