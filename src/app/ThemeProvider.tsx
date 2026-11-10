import { createContext, useContext, useEffect, useRef, useState, type ReactNode } from "react";
import { listen } from "@tauri-apps/api/event";
import { callCommand, isTauriRuntime, normalizeAppError } from "../lib/api";

export const themePresets = [
  { value: "ink_blue", label: "墨蓝", colors: ["#171A21", "#1E222B", "#B4D5FD", "#DEE3EB", "#AAB3C2"] },
  { value: "wisteria", label: "藤紫", colors: ["#15131C", "#211E2A", "#B5A0FF", "#F0EDFF", "#BBB5D1"] },
  { value: "berry", label: "莓粉", colors: ["#171218", "#231C25", "#F18BB5", "#F7EDF4", "#CBB7C6"] },
  { value: "amber", label: "琥珀烛光 · 深夜助眠", colors: ["#151210", "#211C18", "#E5B887", "#F5ECE3", "#B8ABA0"] },
] as const;
export type ThemePreset = typeof themePresets[number]["value"] | "ink_blue_light" | "ink_blue_system";
const CACHE_KEY = "asmr-theme-preset";
export function validTheme(value: unknown): value is ThemePreset {
  return value === "ink_blue_light" || value === "ink_blue_system" || themePresets.some(preset => preset.value === value);
}
function applyTheme(preset: ThemePreset) {
  document.documentElement.dataset.theme = preset;
  // Cache only avoids a bright first frame; SQLite remains the source of truth.
  try { localStorage.setItem(CACHE_KEY, preset); } catch { /* Storage may be unavailable. */ }
}
export function initializeTheme() {
  let preset: ThemePreset = "wisteria";
  try { const cached = localStorage.getItem(CACHE_KEY); if (validTheme(cached)) preset = cached; } catch { /* Use default. */ }
  applyTheme(preset);
}

interface ThemeState {
  preset: ThemePreset; ready: boolean; pending: boolean; error: string;
  save: (preset: ThemePreset) => Promise<void>;
}
const ThemeContext = createContext<ThemeState>({ preset: "wisteria", ready: false, pending: false, error: "", save: async () => {} });
export const useTheme = () => useContext(ThemeContext);

export function ThemeProvider({ children }: { children: ReactNode }) {
  const [preset, setPreset] = useState<ThemePreset>(() => {
    const value = document.documentElement.dataset.theme;
    return validTheme(value) ? value : "wisteria";
  });
  const [ready, setReady] = useState(false);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState("");
  const busy = useRef(false);
  const revision = useRef(0);
  const accept = (value: unknown) => {
    if (!validTheme(value)) return;
    applyTheme(value); setPreset(value); setReady(true); setError("");
  };
  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    const refresh = async () => {
      if (busy.current) return;
      const id = ++revision.current;
      try {
        const saved = await callCommand<ThemePreset>("get_theme_preset");
        if (!disposed && id === revision.current) accept(saved);
      } catch (e) {
        if (!disposed && id === revision.current) setError(normalizeAppError(e).message);
      }
    };
    void (async () => {
      if (!isTauriRuntime()) { accept(preset); return; }
      try {
        const stop = await listen<ThemePreset>("theme:updated", event => {
          if (!disposed) { ++revision.current; accept(event.payload); }
        });
        if (disposed) { stop(); return; }
        unlisten = stop;
      } catch (e) { if (!disposed) setError(normalizeAppError(e).message); }
      if (!disposed) await refresh();
    })();
    const onFocus = () => { if (isTauriRuntime()) void refresh(); };
    window.addEventListener("focus", onFocus);
    return () => { disposed = true; unlisten?.(); window.removeEventListener("focus", onFocus); };
    // This subscription is per window; selection changes must not recreate it.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  async function save(next: ThemePreset) {
    if (busy.current || !validTheme(next)) return;
    busy.current = true; ++revision.current; setPending(true); setError("");
    try {
      const saved = isTauriRuntime() ? await callCommand<ThemePreset>("set_theme_preset", { preset: next }) : next;
      accept(saved);
    } catch (e) { setError(normalizeAppError(e).message); }
    finally { busy.current = false; setPending(false); }
  }
  return <ThemeContext.Provider value={{ preset, ready, pending, error, save }}>{children}</ThemeContext.Provider>;
}
