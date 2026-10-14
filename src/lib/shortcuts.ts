import catalog from "./commandCatalog.json";

export const commands = catalog;
export type ShortcutBindings = Record<string, string | null>;
export const defaultBindings: ShortcutBindings = Object.fromEntries(commands.map(command => [command.id, command.defaultBinding]));
const reserved = new Set(["Ctrl+KeyW", "Ctrl+KeyQ", "Ctrl+KeyR", "Ctrl+KeyL", "Ctrl+KeyN", "Ctrl+KeyT", "Ctrl+KeyP", "Ctrl+KeyO", "Ctrl+KeyS", "Ctrl+KeyF", "Ctrl+KeyC", "Ctrl+KeyV", "Ctrl+KeyX", "Ctrl+KeyZ", "Ctrl+KeyA", "Ctrl+KeyY", "Ctrl+Shift+KeyR", "Ctrl+Shift+KeyT", "Ctrl+Shift+KeyN", "Ctrl+Shift+KeyI", "Ctrl+Shift+Delete", "Alt+F4", "Ctrl+Alt+Delete"]);
export function bindingError(binding: string): string | null {
  if (reserved.has(binding)) return "该组合保留给系统或常用编辑操作，请换一个快捷键。";
  const parts = binding.split("+"); const code = parts.pop() ?? "";
  if (!/^(Key[A-Z]|Digit[0-9]|Arrow(Left|Right|Up|Down)|Space|Comma|Period|F([1-9]|10))$/.test(code)) return "不支持此按键；Esc、Tab、Enter 和系统媒体键保留原有用途。";
  if (parts.some(part => !["Ctrl", "Alt", "Shift"].includes(part)) || new Set(parts).size !== parts.length || parts.join("+") !== ["Ctrl", "Alt", "Shift"].filter(part => parts.includes(part)).join("+")) return "快捷键格式无效。";
  if (parts.includes("Alt") && !parts.includes("Ctrl")) return "Alt 组合可能由系统占用，请同时使用 Ctrl。";
  if (!parts.includes("Ctrl") && !/^(Space|Arrow(Left|Right|Up|Down)|F([1-9]|10))$/.test(code)) return "字母、数字和标点需搭配 Ctrl，避免影响正常输入。";
  if (["F5", "F10"].includes(code) && parts.length === 0) return "此功能键保留给窗口操作，请搭配 Ctrl。";
  return null;
}
export function validateBindings(bindings: ShortcutBindings): string | null {
  const used = new Map<string, string>();
  for (const [id, binding] of Object.entries(bindings)) {
    const command = commands.find(item => item.id === id);
    if (!command) return "存在未知命令，请重新加载快捷键。";
    if (binding === null) continue;
    if (typeof binding !== "string") return "快捷键格式无效。";
    const error = bindingError(binding); if (error) return error;
    if (used.has(binding)) return `与「${used.get(binding)}」冲突，请先修改或清除另一项。`;
    used.set(binding, command.label);
  }
  return null;
}
export function eventBinding(event: Pick<KeyboardEvent, "code" | "ctrlKey" | "altKey" | "shiftKey" | "metaKey" | "isComposing" | "keyCode" | "getModifierState">): string | null {
  if (event.isComposing || event.keyCode === 229 || event.metaKey || event.getModifierState?.("AltGraph") || /^(Control|Alt|Shift|Meta)/.test(event.code)) return null;
  return [...(event.ctrlKey ? ["Ctrl"] : []), ...(event.altKey ? ["Alt"] : []), ...(event.shiftKey ? ["Shift"] : []), event.code].join("+");
}
export function formatBinding(binding?: string | null): string {
  if (!binding) return "未绑定";
  return binding.replace(/Key([A-Z])/g, "$1").replace(/Digit([0-9])/g, "$1")
    .replace("ArrowLeft", "←").replace("ArrowRight", "→").replace("ArrowUp", "↑").replace("ArrowDown", "↓").replace("Comma", ",").replace("Period", ".").replaceAll("+", " + ");
}
export function shortcutTitle(label: string, binding?: string | null) { return binding ? `${label} (${formatBinding(binding)})` : label; }
export function shortcutTargetBlocked(target: EventTarget | null): boolean {
  if (document.querySelector('[aria-modal="true"], dialog[open]')) return true;
  return target instanceof Element && !!target.closest('input, textarea, select, [contenteditable]:not([contenteditable="false"]), [data-select-control], [role="listbox"], [role="menu"], [role="dialog"], [role="slider"], [data-shortcut-editor]');
}
