import { createContext, useContext, useEffect, useState, type ReactNode } from "react";

export const fontPresets = [
  { value: "noto", label: "Noto Sans SC · 内置", localNames: [] },
  { value: "yahei", label: "微软雅黑 · 屏幕阅读", localNames: ["Microsoft YaHei UI", "Microsoft YaHei"] },
  { value: "dengxian", label: "等线 · 清爽简洁", localNames: ["DengXian"] },
] as const;
export type InterfaceFont = typeof fontPresets[number]["value"];
export interface Typography { font: InterfaceFont; weight: 400 | 500 | 600 }
export const TYPOGRAPHY_KEY = "asmr-interface-typography";
const defaults: Typography = { font: "noto", weight: 500 };
export function validTypography(value: unknown): value is Typography {
  if (!value || typeof value !== "object") return false;
  const candidate = value as Partial<Typography>;
  return fontPresets.some(item => item.value === candidate.font) && [400, 500, 600].includes(candidate.weight ?? 0);
}
function readTypography(): Typography {
  try {
    const saved: unknown = JSON.parse(localStorage.getItem(TYPOGRAPHY_KEY) ?? "null");
    return validTypography(saved) ? saved : defaults;
  } catch { return defaults; }
}
function applyTypography(value: Typography) {
  document.documentElement.dataset.interfaceFont = value.font;
  document.documentElement.style.setProperty("--font-weight-body", String(value.weight));
}
export function initializeTypography() { applyTypography(readTypography()); }

const TypographyContext = createContext({ value: defaults, fontRevision: 0, error: "", save: (_value: Typography) => {} });
export const useTypography = () => useContext(TypographyContext);

/** Device-local presentation, shared by same-origin WebViews. No player commands. */
export function TypographyProvider({ children }: { children: ReactNode }) {
  const [value, setValue] = useState(readTypography);
  const [error, setError] = useState("");
  const [fontRevision, setFontRevision] = useState(0);
  useEffect(() => {
    const fonts = document.fonts;
    if (!fonts) return;
    let disposed = false;
    // Downloaded fonts can change measured text widths without changing the selection.
    const loaded = () => { if (!disposed) setFontRevision(revision => revision + 1); };
    void fonts.ready.then(loaded);
    fonts.addEventListener("loadingdone", loaded);
    return () => { disposed = true; fonts.removeEventListener("loadingdone", loaded); };
  }, []);
  useEffect(() => {
    const refresh = () => {
      const next = readTypography();
      applyTypography(next); setValue(next); setError("");
    };
    const onStorage = (event: StorageEvent) => {
      if (event.key === TYPOGRAPHY_KEY || event.key === null) refresh();
    };
    refresh();
    window.addEventListener("storage", onStorage);
    window.addEventListener("focus", refresh);
    return () => { window.removeEventListener("storage", onStorage); window.removeEventListener("focus", refresh); };
  }, []);
  function save(next: Typography) {
    if (!validTypography(next)) return;
    try {
      localStorage.setItem(TYPOGRAPHY_KEY, JSON.stringify(next));
      applyTypography(next); setValue(next); setError("");
    } catch { setError("字体设置未能保存，请重试。"); }
  }
  return <TypographyContext.Provider value={{ value, fontRevision, error, save }}>{children}</TypographyContext.Provider>;
}
