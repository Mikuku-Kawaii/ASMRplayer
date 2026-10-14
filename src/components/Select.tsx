import { useEffect, useId, useLayoutEffect, useRef, useState } from "react";
import type { AriaAttributes, CSSProperties, KeyboardEvent } from "react";
import { createPortal } from "react-dom";
import { Check, ChevronDown } from "lucide-react";

export interface SelectOption { value: string; label: string; disabled?: boolean }
export interface SelectProps extends AriaAttributes {
  id?: string;
  value: string;
  options: readonly SelectOption[];
  onValueChange: (value: string) => void;
  disabled?: boolean;
  size?: "regular" | "compact";
  variant?: "field" | "chip";
  className?: string;
  placeholder?: string;
}

// Focus stays on the combobox: Tab follows its normal document order even though
// the listbox is portaled. Highlighting never commits a value or saves settings.
export function Select({ id: suppliedId, value, options, onValueChange, disabled = false,
  size = "regular", variant = "field", className = "", placeholder = "请选择", ...aria }: SelectProps) {
  const generatedId = useId();
  const id = suppliedId ?? `select-${generatedId}`;
  const listId = `${id}-list`;
  const trigger = useRef<HTMLButtonElement>(null);
  const panel = useRef<HTMLDivElement>(null);
  const [phase, setPhase] = useState<"closed" | "open" | "closing">("closed");
  const open = phase === "open";
  const [active, setActive] = useState<string | null>(null);
  const [keyboard, setKeyboard] = useState(false);
  const [position, setPosition] = useState<CSSProperties>({ visibility: "hidden" });
  const [side, setSide] = useState("bottom");
  const search = useRef({ text: "", time: 0 });
  const selected = options.find(option => option.value === value);
  const enabled = options.filter(option => !option.disabled);
  const activeIndex = options.findIndex(option => option.value === active && !option.disabled);
  const close = (restore = false) => {
    setPhase(current => current === "open" ? "closing" : current);
    search.current = { text: "", time: 0 };
    if (restore) trigger.current?.focus({ preventScroll: true });
  };
  const show = (fromKeyboard: boolean, edge?: "first" | "last") => {
    if (disabled) return;
    setKeyboard(fromKeyboard);
    setActive((edge === "last" ? enabled.at(-1) : edge === "first" ? enabled[0]
      : enabled.find(option => option.value === value) ?? enabled[0])?.value ?? null);
    setPhase("open");
  };
  const commit = (option: SelectOption) => {
    if (disabled || option.disabled || !open) return;
    close(true);
    if (option.value !== value) onValueChange(option.value);
  };

  useEffect(() => {
    if (phase !== "closing") return;
    const timer = setTimeout(() => setPhase("closed"), window.matchMedia?.("(prefers-reduced-motion: reduce)").matches ? 0 : 180);
    return () => clearTimeout(timer);
  }, [phase]);
  useEffect(() => { if (disabled) setPhase("closed"); }, [disabled]);
  useEffect(() => {
    if (!open) return;
    const outside = (event: Event) => {
      const node = event.target as Node;
      if (!trigger.current?.contains(node) && !panel.current?.contains(node)) close();
    };
    const blur = () => close();
    document.addEventListener("pointerdown", outside, true);
    document.addEventListener("focusin", outside);
    window.addEventListener("blur", blur);
    return () => {
      document.removeEventListener("pointerdown", outside, true);
      document.removeEventListener("focusin", outside);
      window.removeEventListener("blur", blur);
    };
  }, [open]);

  useLayoutEffect(() => {
    if (!open || !panel.current || !trigger.current) return;
    const menu = panel.current;
    // Manual popovers enter the browser top layer without native select styling.
    // Portaling into the nearest dialog also preserves modal/inert containment.
    menu.setAttribute("popover", "manual");
    menu.showPopover?.();
    const place = () => {
      const rect = trigger.current!.getBoundingClientRect();
      const viewport = window.visualViewport;
      const left = viewport?.offsetLeft ?? 0, top = viewport?.offsetTop ?? 0;
      const width = viewport?.width ?? window.innerWidth, height = viewport?.height ?? window.innerHeight;
      const menuWidth = Math.min(Math.max(rect.width, 200), Math.max(0, width - 16));
      const below = top + height - rect.bottom - 14, above = rect.top - top - 14;
      const desired = Math.min(menu.scrollHeight ? menu.scrollHeight + 2 : options.length * 36 + 14, 300);
      const up = below < desired && above > below;
      const maxHeight = Math.max(0, Math.min(300, up ? above : below));
      setSide(up ? "top" : "bottom");
      setPosition({ left: Math.max(left + 8, Math.min(rect.left, left + width - menuWidth - 8)),
        top: up ? Math.max(top + 8, rect.top - 6 - Math.min(desired, maxHeight)) : rect.bottom + 6,
        width: menuWidth, maxHeight, visibility: "visible" });
    };
    place();
    const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(place);
    observer?.observe(trigger.current); observer?.observe(menu);
    window.addEventListener("resize", place);
    document.addEventListener("scroll", place, true);
    window.visualViewport?.addEventListener("resize", place);
    window.visualViewport?.addEventListener("scroll", place);
    return () => {
      observer?.disconnect(); window.removeEventListener("resize", place);
      document.removeEventListener("scroll", place, true);
      window.visualViewport?.removeEventListener("resize", place);
      window.visualViewport?.removeEventListener("scroll", place);
    };
  }, [open, options.length]);
  useLayoutEffect(() => {
    const menu = panel.current;
    const option = menu?.querySelector<HTMLElement>(`[data-index="${activeIndex}"]`);
    if (!open || !menu || !option) return;
    const box = menu.getBoundingClientRect(), row = option.getBoundingClientRect();
    if (row.top < box.top + 6) menu.scrollTop -= box.top + 6 - row.top;
    else if (row.bottom > box.bottom - 6) menu.scrollTop += row.bottom - box.bottom + 6;
  }, [open, activeIndex, position.top, position.maxHeight]);

  function keyDown(event: KeyboardEvent<HTMLButtonElement>) {
    // Do not let Space/seek arrows/typeahead reach the global player shortcuts.
    if (event.key === "Tab") { close(); return; }
    if (event.key === "Escape" && !open) return;
    event.stopPropagation();
    if (disabled || event.nativeEvent.isComposing) return;
    const key = event.key;
    if (["ArrowDown", "ArrowUp", "Home", "End"].includes(key)) {
      event.preventDefault(); setKeyboard(true);
      if (!open) { show(true, key === "Home" ? "first" : key === "End" ? "last" : undefined); return; }
      const index = enabled.findIndex(option => option.value === active);
      const next = key === "Home" ? 0 : key === "End" ? enabled.length - 1
        : (index + (key === "ArrowDown" ? 1 : -1) + enabled.length) % enabled.length;
      setActive(enabled[next]?.value ?? null);
    } else if (key === "Escape" && open) {
      event.preventDefault(); close(true);
    } else if (key === "Enter" || key === " ") {
      event.preventDefault();
      if (!open) show(true); else if (options[activeIndex]) commit(options[activeIndex]);
    } else if (key.length === 1 && !event.ctrlKey && !event.metaKey && !event.altKey) {
      event.preventDefault(); setKeyboard(true);
      const now = Date.now();
      let query = now - search.current.time > 700 ? key : search.current.text + key;
      search.current = { text: query, time: now };
      if ([...query].every(char => char === query[0])) query = query[0] ?? key;
      const index = enabled.findIndex(option => option.value === (open ? active : value));
      const candidates = query.length === 1 ? [...enabled.slice(index + 1), ...enabled.slice(0, index + 1)] : enabled;
      const match = candidates.find(option => option.label.trim().toLocaleLowerCase().startsWith(query.toLocaleLowerCase()));
      if (!open) show(true);
      if (match) setActive(match.value);
    }
  }

  const host = trigger.current?.closest('[role="dialog"], dialog, [aria-modal="true"]') ?? document.body;
  const glass = Boolean(trigger.current?.closest(".player-glass-panel"));
  return <>
    <button {...aria} id={id} ref={trigger} type="button" role="combobox" data-select-control="true"
      className={`app-select app-select-${size} app-select-${variant} ${variant === "chip" ? "btn-chip" : ""} ${className}`} disabled={disabled} aria-disabled={disabled}
      aria-haspopup="listbox" aria-expanded={open} aria-controls={phase !== "closed" ? listId : undefined}
      aria-activedescendant={open && activeIndex >= 0 ? `${listId}-${activeIndex}` : undefined}
      title={selected?.label ?? placeholder} onKeyDown={keyDown}
      onClick={() => open ? close() : show(false)}>
      <span className="app-select-value">{selected?.label ?? placeholder}</span><ChevronDown size={16} aria-hidden="true" />
    </button>
    {phase !== "closed" && createPortal(<div ref={panel} id={listId} role="listbox"
      aria-label={aria["aria-label"]} aria-labelledby={aria["aria-label"] ? undefined : aria["aria-labelledby"] ?? id}
      aria-hidden={!open || undefined} data-side={side} data-keyboard={keyboard}
      className={`app-select-menu ${glass ? "player-glass-panel" : ""} ${open ? "is-open" : "is-closing"}`} style={position}
      onPointerDown={event => event.preventDefault()} onClick={event => event.stopPropagation()}>
      {options.map((option, index) => <div key={option.value} id={`${listId}-${index}`} role="option"
        data-index={index} aria-selected={option.value === value} aria-disabled={option.disabled || undefined}
        className={`app-select-option ${option.value === active ? "is-active" : ""}`}
        onPointerMove={() => { if (!option.disabled) { setKeyboard(false); setActive(option.value); } }}
        onClick={() => commit(option)}>
        <span>{option.label}</span><span className="app-select-check" aria-hidden="true">{option.value === value && <Check size={15} />}</span>
      </div>)}
      {options.length === 0 && <div className="app-select-empty">暂无可选项</div>}
    </div>, host)}
  </>;
}
