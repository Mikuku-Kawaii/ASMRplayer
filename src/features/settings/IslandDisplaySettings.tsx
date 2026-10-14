import { Select } from "../../components/Select";
import { useCallback, useEffect, useRef, useState } from "react";
import { Minus, Plus } from "lucide-react";
import { callCommand, IslandPreferences, normalizeAppError } from "../../lib/api";

export function IslandDisplaySettings() {
  const [preferences, setPreferences] = useState<IslandPreferences | null>(null);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState("");
  const request = useRef(0);
  const changing = useRef(false);
  const invalidate = useCallback(() => { ++request.current; }, []);
  const refresh = useCallback(async () => {
    if (changing.current) return;
    const id = ++request.current;
    try {
      const next = await callCommand<IslandPreferences>("get_island_preferences");
      if (id === request.current) { setPreferences(next); setError(""); }
    } catch (e) { if (id === request.current) setError(normalizeAppError(e).message); }
  }, []);
  useEffect(() => {
    void refresh();
    const onFocus = () => void refresh();
    window.addEventListener("focus", onFocus);
    return () => { invalidate(); window.removeEventListener("focus", onFocus); };
  }, [refresh, invalidate]);
  async function change(value: { compactScale?: number; expandedScale?: number; monitorId?: string }) {
    if (changing.current) return;
    changing.current = true;
    const id = ++request.current;
    setPending(true); setError("");
    try {
      const next = await callCommand<IslandPreferences>("set_island_preferences", value);
      if (id === request.current) setPreferences(next);
    } catch (e) { if (id === request.current) setError(normalizeAppError(e).message); }
    finally { changing.current = false; if (id === request.current) setPending(false); }
  }
  const disconnected = preferences?.monitorId && !preferences.monitors.some(m => m.id === preferences.monitorId);
  return <>
    <div className="settings-row">
      <div className="settings-label-block">
        <label htmlFor="island-monitor">显示屏幕</label>
        <span className="settings-field-hint">切换后移至屏幕顶部居中，断开屏幕时自动回到主屏</span>
      </div>
      <div className="island-display-actions">
        <Select id="island-monitor" value={preferences?.monitorId ?? ""} disabled={!preferences || pending} onValueChange={monitorId => void change({ monitorId })}
          options={[
            { value: "", label: "跟随主屏幕" },
            ...(disconnected ? [{ value: preferences!.monitorId, label: "已断开的屏幕（当前使用主屏幕）" }] : []),
            ...(preferences?.monitors.map((m, i) => ({ value: m.id, label: `屏幕 ${i + 1}${m.primary ? " · 主屏幕" : ""} · ${m.width} × ${m.height} · ${Math.round(m.scaleFactor * 100)}%` })) ?? []),
          ]} />
        <button type="button" className="btn btn-ghost" disabled={pending} onClick={() => void refresh()}>刷新屏幕</button>
      </div>
    </div>
    {([["compactScale", "缩小状态大小"], ["expandedScale", "展开状态大小"]] as const).map(([key, label]) => {
      const value = preferences?.[key] ?? 1;
      return <div className="settings-row" key={key}>
        <div className="settings-label-block"><span id={`island-${key}-label`}>{label}</span>
          <span className="settings-field-hint">1.0 为原“小”档 · 每次调整 0.1</span></div>
        <div className="island-scale-stepper" role="group" aria-labelledby={`island-${key}-label`}>
          <button type="button" aria-label={`减小${label}`} title={`减小${label} 0.1`} disabled={!preferences || pending || value <= 0.5}
            onClick={() => void change({ [key]: Math.round((value - 0.1) * 10) / 10 })}><Minus size={16} /></button>
          <output aria-label={label} aria-live="polite" className="tabular-nums">{preferences ? value.toFixed(2) : "—"}</output>
          <button type="button" aria-label={`增大${label}`} title={`增大${label} 0.1`} disabled={!preferences || pending || value >= 2}
            onClick={() => void change({ [key]: Math.round((value + 0.1) * 10) / 10 })}><Plus size={16} /></button>
        </div>
      </div>;
    })}
    {error && <p className="settings-field-hint" role="alert">{error}</p>}
  </>;
}
