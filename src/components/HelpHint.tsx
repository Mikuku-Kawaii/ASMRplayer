import { useEffect, useId, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { CircleAlert } from "lucide-react";

/** Inline help stays outside the form label so opening it never changes a setting. */
export function HelpHint({ label, text }: { label: string; text: ReactNode }) {
  const id = useId();
  const anchor = useRef<HTMLButtonElement>(null);
  const tip = useRef<HTMLDivElement>(null);
  const timer = useRef<ReturnType<typeof setTimeout>>();
  const hovered = useRef(false), focused = useRef(false);
  const [open, setOpen] = useState(false);
  const clear = () => clearTimeout(timer.current);
  const enter = () => { clear(); hovered.current = true; setOpen(true); };
  const leave = () => {
    hovered.current = false;
    clear();
    timer.current = setTimeout(() => { if (!focused.current && !hovered.current) setOpen(false); }, 120);
  };
  useEffect(() => () => clearTimeout(timer.current), []);
  useLayoutEffect(() => {
    if (!open || !tip.current || !anchor.current) return;
    const panel = tip.current;
    panel.setAttribute("popover", "manual");
    panel.showPopover?.();
    const position = () => {
      if (!anchor.current) return;
      const rect = anchor.current.getBoundingClientRect();
      const width = document.documentElement.clientWidth, height = window.innerHeight;
      panel.style.maxWidth = `${Math.max(0, width - 16)}px`;
      panel.style.maxHeight = `${Math.max(0, height - 16)}px`;
      const bounds = panel.getBoundingClientRect();
      panel.style.left = `${Math.max(8, Math.min(rect.left, width - bounds.width - 8))}px`;
      const below = rect.bottom + 6;
      panel.style.top = `${Math.max(8, Math.min(below + bounds.height <= height - 8 ? below : rect.top - bounds.height - 6, height - bounds.height - 8))}px`;
    };
    const escape = (event: KeyboardEvent) => {
      if (event.key === "Escape") { event.preventDefault(); event.stopPropagation(); setOpen(false); }
    };
    position();
    window.addEventListener("resize", position);
    window.addEventListener("scroll", position, true);
    document.addEventListener("keydown", escape, true);
    return () => {
      window.removeEventListener("resize", position);
      window.removeEventListener("scroll", position, true);
      document.removeEventListener("keydown", escape, true);
      panel.hidePopover?.();
    };
  }, [open]);
  return <>
    <button ref={anchor} type="button" className="setting-help-button" aria-label={`${label}说明`} title={`查看${label}说明`}
      aria-describedby={open ? id : undefined} onMouseEnter={enter} onMouseLeave={leave}
      onFocus={() => { clear(); focused.current = true; setOpen(true); }}
      onBlur={() => { focused.current = false; if (!hovered.current) { clear(); setOpen(false); } }}
      onClick={() => setOpen(true)}><CircleAlert size={16} aria-hidden="true" /></button>
    {open && createPortal(<div ref={tip} id={id} role="tooltip" className="setting-help-tooltip" onMouseEnter={enter} onMouseLeave={leave}>{text}</div>, anchor.current?.closest("dialog, [role='dialog']") ?? document.body)}
  </>;
}

export function SettingLabel({ label, htmlFor, help }: { label: string; htmlFor?: string; help: string }) {
  return <span className="setting-label-help">{htmlFor ? <label htmlFor={htmlFor}>{label}</label> : <span>{label}</span>}<HelpHint label={label} text={help} /></span>;
}
