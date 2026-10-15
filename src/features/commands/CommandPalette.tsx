import { useEffect, useId, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { Search, X } from "lucide-react";
import { commands, formatBinding } from "../../lib/shortcuts";
import { normalizeAppError } from "../../lib/api";
import { useShortcuts } from "./ShortcutProvider";

export function CommandPalette({ onClose, onExecute, disabledReason }: {
  onClose: () => void; onExecute: (id: string) => Promise<void>; disabledReason: (id: string) => string | null;
}) {
  const { bindings, loadError } = useShortcuts();
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const locked = useRef(false);
  const dialog = useRef<HTMLDivElement>(null);
  const input = useRef<HTMLInputElement>(null);
  const close = useRef(onClose); close.current = onClose;
  const id = useId();
  const terms = query.trim().toLowerCase().split(/\s+/);
  const results = commands.filter(command => command.id !== "command_search" && terms.every(term => `${command.label} ${command.keywords} ${command.category}`.toLowerCase().includes(term)));
  const selected = results[Math.min(active, Math.max(0, results.length - 1))];
  useEffect(() => {
    const previous = document.activeElement as HTMLElement | null;
    const shell = document.querySelector<HTMLElement>(".app-shell"); const inert = shell?.inert;
    if (shell) shell.inert = true;
    const overflow = document.body.style.overflow; document.body.style.overflow = "hidden";
    document.body.classList.add("command-palette-open"); input.current?.focus();
    const focus = (event: FocusEvent) => { if (!dialog.current?.contains(event.target as Node)) input.current?.focus(); };
    document.addEventListener("focusin", focus);
    return () => {
      document.removeEventListener("focusin", focus);
      document.body.style.overflow = overflow; document.body.classList.remove("command-palette-open");
      if (shell) shell.inert = inert ?? false;
      if (previous?.isConnected) previous.focus();
    };
  }, []);
  useEffect(() => { if (selected) document.getElementById(`${id}-${selected.id}`)?.scrollIntoView?.({ block: "nearest" }); }, [selected, id]);
  async function execute(commandId: string) {
    if (locked.current) return;
    const reason = disabledReason(commandId); if (reason) { setError(reason); return; }
    locked.current = true; setBusy(true); setError("");
    try { await onExecute(commandId); close.current(); }
    catch (e) { const error = normalizeAppError(e); setError([error.message, error.hint].filter(Boolean).join(" ")); }
    finally { locked.current = false; setBusy(false); }
  }
  return createPortal(<div className="command-palette-backdrop" onMouseDown={event => { if (event.target === event.currentTarget) onClose(); }}>
    <div className="command-palette modal-card" role="dialog" aria-modal="true" aria-labelledby={`${id}-title`} ref={dialog} onKeyDown={event => {
      event.stopPropagation();
      if (event.nativeEvent.isComposing || event.keyCode === 229) return;
      if (event.key === "Escape") { event.preventDefault(); onClose(); }
      if (event.key === "Tab") {
        const elements = Array.from(dialog.current?.querySelectorAll<HTMLElement>('input:not(:disabled),button:not(:disabled):not([tabindex="-1"])') ?? []);
        const index = elements.indexOf(document.activeElement as HTMLElement);
        event.preventDefault(); elements[(index + (event.shiftKey ? -1 : 1) + elements.length) % elements.length]?.focus();
      }
    }}>
      <header className="command-palette-header"><h2 id={`${id}-title`}><Search size={18} /><span className="ui-icon-label">搜索命令</span></h2><button className="btn-icon" aria-label="关闭命令搜索" title="关闭命令搜索 (Esc)" onClick={onClose}><X size={17} /></button></header>
      <input ref={input} role="combobox" className="input-text" aria-label="搜索操作" aria-autocomplete="list" aria-expanded="true" aria-controls={`${id}-results`} aria-activedescendant={selected ? `${id}-${selected.id}` : undefined}
        value={query} disabled={busy} placeholder="搜索播放、队列、书签、设置…" onChange={event => { setQuery(event.target.value); setActive(0); setError(""); }} onKeyDown={event => {
          if (event.nativeEvent.isComposing || event.keyCode === 229 || event.repeat) return;
          if (["ArrowDown", "ArrowUp"].includes(event.key)) { event.preventDefault(); setActive(value => (value + (event.key === "ArrowDown" ? 1 : -1) + results.length) % Math.max(1, results.length)); }
          if (event.key === "Enter" && selected) { event.preventDefault(); void execute(selected.id); }
        }} />
      <div className="command-palette-results" role="listbox" aria-label="匹配的命令" id={`${id}-results`}>
        {results.map((command, index) => <button key={command.id} id={`${id}-${command.id}`} type="button" role="option" aria-selected={selected?.id === command.id} aria-disabled={!!disabledReason(command.id) || busy} tabIndex={-1}
          onMouseEnter={() => setActive(index)} onMouseDown={event => event.preventDefault()} onClick={() => void execute(command.id)}>
          <span><strong>{command.label}</strong><small>{disabledReason(command.id) ?? `${command.category} · 应用内`}</small></span><kbd>{formatBinding(bindings[command.id])}</kbd>
        </button>)}
      </div>
      {results.length === 0 && <p className="settings-field-hint" role="status">没有匹配命令，请换一个关键词。</p>}
      {loadError && <p className="shortcut-error" role="alert">{loadError}</p>}
      {error && <p className="shortcut-error" role="alert">{error}</p>}
      <footer className="settings-field-hint">{busy ? "正在执行…" : "↑ ↓ 选择 · Enter 执行 · Esc 关闭"}</footer>
    </div>
  </div>, document.body);
}
