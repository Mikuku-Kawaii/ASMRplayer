import { useEffect, useState } from "react";
import { Copy, ClipboardPaste, Scissors } from "lucide-react";
import { ContextMenu } from "./ContextMenu";
import { Toast } from "./Toast";

type TextField = HTMLInputElement | HTMLTextAreaElement;
type SelectionTarget = { element: HTMLElement; value: string; start: number; end: number; range: Range | null; selected: string; readOnly: boolean; x: number; y: number };
function isField(element: HTMLElement): element is TextField {
  return element instanceof HTMLInputElement || element instanceof HTMLTextAreaElement;
}
function restore(target: SelectionTarget) {
  const { element } = target;
  if (!element.isConnected) return false;
  element.focus({ preventScroll: true });
  if (isField(element)) {
    if (element.value !== target.value) return false;
    if (element.selectionStart !== null) element.setSelectionRange(target.start, target.end);
  } else if (target.range) {
    if (element.textContent !== target.value) return false;
    const selection = window.getSelection(); selection?.removeAllRanges(); selection?.addRange(target.range);
  }
  return true;
}

/** Preserve app-owned context menus; never expose WebView navigation/devtools menus. */
export function TextContextMenu() {
  const [target, setTarget] = useState<SelectionTarget | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<{ id: number; message: string } | null>(null);
  useEffect(() => {
    const editable = (event: MouseEvent) => {
      const element = event.target instanceof Element ? event.target.closest<HTMLElement>("input, textarea, [contenteditable]") : null;
      if (!element) return;
      if (element instanceof HTMLInputElement && !["text", "search", "url", "tel", "email", "password"].includes(element.type)) return;
      if (!isField(element) && !element.isContentEditable) return;
      event.preventDefault(); event.stopPropagation();
      if (isField(element) && element.disabled) { setTarget(null); return; }
      const selection = window.getSelection();
      const start = isField(element) ? element.selectionStart ?? element.value.length : 0;
      const end = isField(element) ? element.selectionEnd ?? start : 0;
      const range = !isField(element) && selection?.rangeCount && element.contains(selection.anchorNode) ? selection.getRangeAt(0).cloneRange() : null;
      const rect = element.getBoundingClientRect();
      setTarget({ element, start, end, range, value: isField(element) ? element.value : element.textContent ?? "",
        selected: isField(element) ? element.value.slice(start, end) : range?.toString() ?? "",
        readOnly: isField(element) && element.readOnly,
        x: event.clientX || rect.left + 12, y: event.clientY || rect.bottom });
    };
    const suppress = (event: MouseEvent) => { event.preventDefault(); };
    document.addEventListener("contextmenu", editable, true);
    document.addEventListener("contextmenu", suppress);
    return () => { document.removeEventListener("contextmenu", editable, true); document.removeEventListener("contextmenu", suppress); };
  }, []);
  async function perform(action: "copy" | "cut" | "paste") {
    if (!target || busy) return;
    setBusy(true);
    try {
      if (action !== "paste") await navigator.clipboard.writeText(target.selected);
      if (action !== "copy") {
        const text = action === "paste" ? await navigator.clipboard.readText() : "";
        if (!restore(target)) throw new Error("文字已变化，请重新打开菜单");
        const changed = document.execCommand?.(action === "cut" ? "delete" : "insertText", false, text);
        if (!changed) {
          if (!isField(target.element)) throw new Error("暂时无法编辑，请使用键盘快捷键");
          const field = target.element;
          const value = target.value.slice(0, target.start) + text + target.value.slice(target.end);
          const prototype = field instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
          Object.getOwnPropertyDescriptor(prototype, "value")?.set?.call(field, value);
          if (field.selectionStart !== null) field.setSelectionRange(target.start + text.length, target.start + text.length);
          field.dispatchEvent(new Event("input", { bubbles: true }));
        }
      } else restore(target);
      setTarget(null);
    } catch (failure) {
      setError({ id: Date.now(), message: failure instanceof Error && failure.message.startsWith("文字已") ? failure.message : "无法访问剪贴板，请使用 Ctrl+C、Ctrl+X 或 Ctrl+V" });
      setTarget(null);
    } finally { setBusy(false); }
  }
  return <>
    {target && <ContextMenu x={target.x} y={target.y} host={target.element.closest("dialog, [role='dialog']") ?? document.body}
      label="文字编辑菜单" onClose={() => setTarget(null)}>
      <button type="button" role="menuitem" className="m3-menu-item" disabled={busy || target.readOnly || !target.selected} onClick={() => void perform("cut")}><Scissors size={15} /><span className="ui-icon-label">剪切</span></button>
      <button type="button" role="menuitem" className="m3-menu-item" disabled={busy || !target.selected} onClick={() => void perform("copy")}><Copy size={15} /><span className="ui-icon-label">复制</span></button>
      <button type="button" role="menuitem" className="m3-menu-item" disabled={busy || target.readOnly} onClick={() => void perform("paste")}><ClipboardPaste size={15} /><span className="ui-icon-label">粘贴</span></button>
    </ContextMenu>}
    {error && <Toast key={error.id} message={error.message} onClose={() => setError(null)} />}
  </>;
}
