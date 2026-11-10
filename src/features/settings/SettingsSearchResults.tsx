import { useEffect, useState, type RefObject } from "react";
import { indexSettings, matchSettings, type SettingsCategory, type SettingsSearchEntry } from "./settingsSearch";

export function SettingsSearchResults({ root, categories, query, onSelect }: {
  root: RefObject<HTMLDivElement>; categories: readonly SettingsCategory[]; query: string;
  onSelect: (entry: SettingsSearchEntry) => void;
}) {
  const [entries, setEntries] = useState<SettingsSearchEntry[]>([]);
  useEffect(() => {
    const element = root.current;
    if (!element) return;
    let frame = 0;
    const update = () => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => setEntries(indexSettings(element, categories)));
    };
    update();
    const observer = new MutationObserver(update);
    observer.observe(element, { childList: true, subtree: true, characterData: true });
    return () => { cancelAnimationFrame(frame); observer.disconnect(); };
  }, [root, categories]);
  const matches = matchSettings(entries, query);
  return <section className="settings-search-results" aria-label="设置搜索结果" onKeyDown={event => {
    if (event.nativeEvent.isComposing || event.keyCode === 229) return;
    const buttons = Array.from(event.currentTarget.querySelectorAll<HTMLButtonElement>(".settings-search-result"));
    const index = buttons.indexOf(event.target as HTMLButtonElement);
    if (index < 0) return;
    const next = event.key === "ArrowDown" ? Math.min(buttons.length - 1, index + 1)
      : event.key === "ArrowUp" ? Math.max(0, index - 1) : event.key === "Home" ? 0 : event.key === "End" ? buttons.length - 1 : -1;
    if (next >= 0) { event.preventDefault(); event.stopPropagation(); buttons[next]?.focus(); }
  }}>
    <p role="status" className="settings-field-hint">{matches.length ? `找到 ${matches.length} 项设置 · 选择后定位到对应控件` : "没有找到相关设置，试试“字幕”“音频”或“网络”。"}</p>
    {matches.map(entry => <button type="button" key={entry.key} className="settings-search-result" onClick={() => onSelect(entry)}>
      <strong>{entry.label}</strong><span>{entry.path}</span>
    </button>)}
  </section>;
}
