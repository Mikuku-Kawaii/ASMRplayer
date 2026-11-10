export interface SettingsCategory { id: string; title: string; members: string[] }
export interface SettingsSearchEntry {
  key: string; label: string; path: string; category: string; keywords: string;
  element: HTMLElement; target: HTMLElement;
}

const aliases: Record<string, string> = {
  "theme-preset": "主题 配色 色卡 墨蓝 深色 浅色 跟随系统 藤紫 莓粉 琥珀 烛光 深夜",
  "interface-font": "字体 简中 字形 黑体 Noto 微软雅黑 等线",
  "interface-font-weight": "字重 粗细 标准 中等 加粗",
  "subtitle-engine": "ASR SenseVoice Whisper Faster Kotoba 模型 GPU CPU",
  "recognition-model-heading": "模型管理 模型下载 模型安装 模型删除 运行库 CUDA",
  "subtitle-context": "initial_prompt 识别提示",
  "subtitle-glossary": "人名 词汇 术语 候选错词",
  "deepseek-prompt": "全局 prompt 提示词 风格",
  "setting-api": "镜像 网络 API",
};
const text = (node: Element | null) => node?.textContent?.replace(/\s+/g, " ").trim() ?? "";

/** Index labels and section names, never input values, keys, paths or user content. */
export function indexSettings(root: HTMLElement, categories: readonly SettingsCategory[]): SettingsSearchEntry[] {
  const entries: SettingsSearchEntry[] = [];
  const seen = new Set<HTMLElement>();
  root.querySelectorAll<HTMLElement>("label, .setting-label-help > span:first-child, .settings-row > span:first-child, h3, .settings-disclosure-title, [data-settings-search]").forEach(label => {
    if (label.closest("dialog, [role=dialog]")) return;
    const group = label.closest<HTMLElement>("[data-settings-group]")?.dataset.settingsGroup;
    const category = categories.find(item => item.members.includes(group ?? ""));
    if (!category) return;
    const row = label.closest<HTMLElement>(".settings-row, .subtitle-preference-field, .settings-disclosure, .settings-group-card") ?? label;
    const forId = label.getAttribute("for");
    const control = forId ? document.getElementById(forId) : null;
    const heading = label.matches("h3, .settings-disclosure-title");
    const target = control ?? (heading ? label.closest<HTMLElement>("button") ?? label
      : row.querySelector<HTMLElement>("input, textarea, button:not(.setting-help-button), [tabindex]")) ?? label;
    if (seen.has(target)) return;
    // A wrapping label can contain a text area: exclude its current text/value.
    const clone = label.cloneNode(true) as HTMLElement;
    clone.querySelectorAll("input, textarea, button, select").forEach(node => node.remove());
    const title = label.dataset.settingsSearch ?? text(clone);
    if (!title || title.length > 100) return;
    seen.add(target);
    const groups: string[] = [];
    let parent = label.parentElement;
    while (parent && parent !== root) {
      if (parent.matches(".settings-nested-group")) {
        const name = text(parent.querySelector(".recognition-model-summary strong"));
        if (name) groups.unshift(name);
      } else if (parent.matches(".settings-disclosure")) {
        const name = text(parent.querySelector(":scope > .settings-disclosure-trigger .settings-disclosure-title"));
        if (name && name !== title) groups.unshift(name);
      } else if (parent.matches(".settings-group-card")) {
        const name = text(parent.querySelector("h3"));
        if (name && name !== title) groups.unshift(name);
      }
      parent = parent.parentElement;
    }
    const path = [category.title, ...groups].join(" › ");
    entries.push({ key: `${category.id}:${target.id || title}:${entries.length}`, label: title, path,
      category: category.id, keywords: `${title} ${path} ${target.id} ${aliases[target.id] ?? ""}`.toLowerCase(),
      element: heading ? target : row, target });
  });
  return entries;
}

export function matchSettings(entries: SettingsSearchEntry[], query: string) {
  const terms = query.toLowerCase().trim().split(/\s+/).filter(Boolean);
  return terms.length ? entries.filter(entry => terms.every(term => entry.keywords.includes(term) || entry.keywords.replace(/\s+/g, "").includes(term)))
    .sort((a, b) => Number(a.target.matches("h3, .settings-disclosure-trigger")) - Number(b.target.matches("h3, .settings-disclosure-trigger"))) : [];
}

export function revealSetting(entry: SettingsSearchEntry) {
  entry.target.closest("[data-shortcut-editor]")?.querySelector<HTMLButtonElement>("button[aria-expanded=false][aria-controls]")?.click();
  const parents: HTMLElement[] = [];
  for (let node = entry.target.parentElement; node; node = node.parentElement) {
    if (node.matches(".settings-disclosure")) parents.unshift(node);
  }
  for (const parent of parents) {
    parent.querySelector<HTMLButtonElement>(":scope > button[aria-expanded=false]")?.click();
  }
  return requestAnimationFrame(() => {
    entry.element.scrollIntoView?.({ block: "center", behavior: "auto" });
    const target = entry.target.matches(":disabled") ? entry.element : entry.target;
    if (!target.matches("input, textarea, button, a, [tabindex]")) target.tabIndex = -1;
    target.focus({ preventScroll: true });
  });
}
