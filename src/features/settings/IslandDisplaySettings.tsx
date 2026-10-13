import { useCallback, useEffect, useRef, useState } from "react";
import { callCommand, IslandPreferences, IslandSize, normalizeAppError } from "../../lib/api";

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
  async function change(value: { size?: IslandSize; monitorId?: string }) {
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
        <select id="island-monitor" className="input-select" value={preferences?.monitorId ?? ""} disabled={!preferences || pending} onChange={e => void change({ monitorId: e.target.value })}>
          <option value="">跟随主屏幕</option>
          {disconnected && <option value={preferences?.monitorId}>已断开的屏幕（当前使用主屏幕）</option>}
          {preferences?.monitors.map((m, i) => <option key={m.id} value={m.id}>屏幕 {i + 1}{m.primary ? " · 主屏幕" : ""} · {m.width} × {m.height} · {Math.round(m.scaleFactor * 100)}%</option>)}
        </select>
        <button type="button" className="btn btn-ghost" disabled={pending} onClick={() => void refresh()}>刷新屏幕</button>
      </div>
    </div>
    <div className="settings-row">
      <div className="settings-label-block"><span id="island-size-label">灵动岛大小</span><span className="settings-field-hint">随屏幕分辨率与系统缩放自动适配</span></div>
      <div className="island-size-options" role="group" aria-labelledby="island-size-label">
        {([["extra_small", "特小"], ["small", "小"], ["medium", "中"], ["large", "大"], ["extra_large", "特大"]] as const).map(([size, label]) => <button type="button" key={size} className={`btn btn-chip ${preferences?.size === size ? "active" : ""}`} aria-pressed={preferences?.size === size} disabled={!preferences || pending} onClick={() => void change({ size })}>{label}</button>)}
      </div>
    </div>
    {error && <p className="settings-field-hint" role="alert">{error}</p>}
  </>;
}
