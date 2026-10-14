import { createContext, useCallback, useContext, useEffect, useRef, useState, type ReactNode } from "react";
import { callCommand, normalizeAppError } from "../../lib/api";
import { defaultBindings, validateBindings, type ShortcutBindings } from "../../lib/shortcuts";

interface ShortcutState {
  bindings: ShortcutBindings; ready: boolean; saving: boolean; loadError: string;
  save: (bindings: ShortcutBindings) => Promise<void>; reload: () => void;
}
const Context = createContext<ShortcutState>({ bindings: defaultBindings, ready: true, saving: false, loadError: "",
  save: async () => { throw new Error("快捷键设置尚未连接"); }, reload: () => {} });
export const useShortcuts = () => useContext(Context);

export function ShortcutProvider({ children }: { children: ReactNode }) {
  const [bindings, setBindings] = useState(defaultBindings);
  const [ready, setReady] = useState(false);
  const [saving, setSaving] = useState(false);
  const [loadError, setLoadError] = useState("");
  const [revision, setRevision] = useState(0);
  const busy = useRef(false);
  useEffect(() => {
    let active = true; setReady(false);
    callCommand<ShortcutBindings>("get_shortcut_bindings").then(value => {
      if (!value || typeof value !== "object" || Array.isArray(value)) throw new Error("无法读取快捷键设置");
      const next = { ...defaultBindings, ...value };
      const error = validateBindings(next); if (error) throw new Error(error);
      if (active) { setBindings(next); setLoadError(""); }
    }).catch(error => {
      if (active) setLoadError(`${normalizeAppError(error).message}。当前使用${revision ? "上次生效的" : "默认"}应用内快捷键，可重试读取或恢复默认。`);
    }).finally(() => { if (active) setReady(true); });
    return () => { active = false; };
  }, [revision]);
  const save = useCallback(async (next: ShortcutBindings) => {
    if (busy.current || !ready) throw new Error("快捷键正在读取或保存，请稍后重试");
    const error = validateBindings(next); if (error) throw new Error(error);
    busy.current = true; setSaving(true);
    try {
      const saved = await callCommand<ShortcutBindings>("save_shortcut_bindings", { bindings: next });
      if (!saved || validateBindings(saved)) throw new Error("快捷键保存结果无效，请重新读取设置");
      setBindings({ ...defaultBindings, ...saved }); setLoadError("");
    } finally { busy.current = false; setSaving(false); }
  }, [ready]);
  return <Context.Provider value={{ bindings, ready, saving, loadError, save, reload: () => { if (!busy.current) setRevision(value => value + 1); } }}>{children}</Context.Provider>;
}
