import { ReactNode, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";

export function ContextMenu({ x, y, onClose, children, label = "作品操作菜单", host = document.body }: { x: number; y: number; onClose: () => void; children: ReactNode; label?: string; host?: Element }) {
  const menu = useRef<HTMLDivElement>(null);
  const close = useRef(onClose); close.current = onClose;
  const [position, setPosition] = useState({ left: x, top: y });
  useLayoutEffect(() => {
    const element = menu.current!;
    const previousFocus = document.activeElement as HTMLElement | null;
    element.setAttribute("popover", "manual");
    element.showPopover?.();
    const place = () => {
      const viewport = window.visualViewport;
      const left = viewport?.offsetLeft ?? 0, top = viewport?.offsetTop ?? 0;
      const width = viewport?.width ?? window.innerWidth, height = viewport?.height ?? window.innerHeight;
      // offset sizes are unaffected by the menu's opening animation.
      setPosition({ left: Math.max(left + 8, Math.min(x, left + width - element.offsetWidth - 8)), top: Math.max(top + 8, Math.min(y, top + height - element.offsetHeight - 8)) });
    };
    place();
    element.focus({ preventScroll: true });
    const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(place);
    observer?.observe(element);
    const outside = (event: Event) => { if (!element.contains(event.target as Node)) close.current(); };
    const scroll = (event: Event) => { if (!element.contains(event.target as Node)) close.current(); };
    document.addEventListener("pointerdown", outside, true);
    document.addEventListener("scroll", scroll, true);
    window.addEventListener("resize", place);
    window.visualViewport?.addEventListener("resize", place);
    window.visualViewport?.addEventListener("scroll", place);
    return () => {
      observer?.disconnect();
      document.removeEventListener("pointerdown", outside, true);
      document.removeEventListener("scroll", scroll, true);
      window.removeEventListener("resize", place);
      window.visualViewport?.removeEventListener("resize", place);
      window.visualViewport?.removeEventListener("scroll", place);
      if (element.contains(document.activeElement) || document.activeElement === document.body) previousFocus?.focus?.({ preventScroll: true });
    };
  }, [x, y]);
  useLayoutEffect(() => {
    // Switching menu pages can remove the focused action. Keep keyboard focus in the menu.
    if (document.activeElement === document.body) menu.current?.focus({ preventScroll: true });
  }, [children]);
  // clientX/clientY use viewport coordinates; page transforms must not become the fixed containing block.
  return createPortal(<div ref={menu} className="m3-context-menu app-context-menu" role="menu" aria-label={label} tabIndex={-1}
    style={position} onContextMenu={event => event.preventDefault()} onKeyDown={event => {
      event.stopPropagation();
      if (event.key === "Escape" || event.key === "Tab") { if (event.key === "Escape") event.preventDefault(); close.current(); return; }
      if ((event.target as HTMLElement).matches("input,textarea")) return;
      if (!["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) return;
      event.preventDefault();
      const items = Array.from(menu.current!.querySelectorAll<HTMLButtonElement>('button:not(:disabled)'));
      const index = items.indexOf(document.activeElement as HTMLButtonElement);
      const next = event.key === "Home" ? 0 : event.key === "End" ? items.length - 1 : (index + (event.key === "ArrowUp" ? -1 : 1) + items.length) % items.length;
      items[next]?.focus();
    }}>{children}</div>, host);
}
