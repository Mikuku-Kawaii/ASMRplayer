import { SettingsSaveNote } from "./SettingsSaveNote";
import { SettingsDisclosure } from "./SettingsDisclosure";
import { HelpHint } from "../../components/HelpHint";
import { ErrorNotice } from "../../components/ErrorNotice";
import { Select } from "../../components/Select";
import { useCallback, useEffect, useRef, useState } from "react";
import { Minus, Plus } from "lucide-react";
import { callCommand, IslandPreferences, normalizeAppError, subscribeIslandLayout } from "../../lib/api";

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
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void subscribeIslandLayout(() => void refresh()).then(stop => {
      if (disposed) stop(); else unlisten = stop;
    }).catch(() => {});
    const onFocus = () => void refresh();
    window.addEventListener("focus", onFocus);
    return () => { disposed = true; unlisten?.(); invalidate(); window.removeEventListener("focus", onFocus); };
  }, [refresh, invalidate]);
  async function change(value: { compactScale?: number; expandedScale?: number; monitorId?: string; fontSize?: number; fontWeight?: number; subtitlePreference?: "chinese" | "original"; expandMode?: "focus" | "click" }) {
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
    <SettingsSaveNote busy={pending} error={error}>自动保存 · 立即更新桌面灵动岛</SettingsSaveNote>
    <div className="settings-row">
      <span className="setting-label-help"><span id="island-expand-mode-label">展开方式</span><HelpHint label="展开方式" text="聚焦展开：鼠标移入展开，移出收起；拖动时松手后收起。键盘聚焦也可展开。点击展开：点击后保持展开，使用收起按钮或 Esc 收起。" /></span>
      <div className="settings-segmented-control" role="group" aria-labelledby="island-expand-mode-label">
        {([{ value: "focus", label: "聚焦展开" }, { value: "click", label: "点击展开" }] as const).map(({ value, label }) => {
          const selected = value === (preferences?.layout.expandMode ?? "click");
          return <button key={value} type="button" className="btn btn-secondary" aria-pressed={!!preferences && selected} disabled={!preferences || pending}
            onClick={() => { if (!selected) void change({ expandMode: value }); }}>{label}</button>;
        })}
      </div>
    </div>
    <SettingsDisclosure title="屏幕与外观" description="选择显示屏幕，调整灵动岛大小、字号与字重" defaultOpen>
    <div className="settings-row">
      <span className="setting-label-help"><span id="island-subtitle-preference-label">字幕优先级</span><HelpHint label="字幕优先级" text="优先显示所选语言；只有一种字幕时显示现有内容。展开后另一种语言显示在下方。" /></span>
      <div className="settings-segmented-control" role="group" aria-labelledby="island-subtitle-preference-label">
        {([{value:"chinese",label:"优先中文字幕"},{value:"original",label:"优先原文字幕"}] as const).map(({value,label}) => {
          const selected = value === (preferences?.layout.subtitlePreference ?? "original");
          return <button key={value} type="button" className="btn btn-secondary" aria-pressed={!!preferences && selected} disabled={!preferences || pending} onClick={() => { if (!selected) void change({subtitlePreference:value}); }}>{label}</button>;
        })}
      </div>
    </div>
    <div className="settings-row">
      <span className="setting-label-help"><label htmlFor="island-monitor">显示屏幕</label><HelpHint label={"显示屏幕"} text={<>大小、字号与字重按灵动岛当前所在屏幕自动保存。每块屏幕首次使用时沿用原设置，调整一次后即可分别记住；切换或重新连接时恢复该屏幕的设置。断开所选屏幕时使用主屏幕及其设置，已断开屏幕的设置仍保留。</>} /></span>
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
        <span className="setting-label-help"><span id={`island-${key}-label`}>{label}</span><HelpHint label={label} text={<>仅调整当前屏幕，自动保存。1.0 为原“小”档 · 每次调整 0.1</>} /></span>
        <div className="island-scale-stepper" role="group" aria-labelledby={`island-${key}-label`}>
          <button type="button" aria-label={`减小${label}`} title={`减小${label} 0.1`} disabled={!preferences || pending || value <= 0.5}
            onClick={() => void change({ [key]: Math.round((value - 0.1) * 10) / 10 })}><Minus size={16} /></button>
          <output aria-label={label} aria-live="polite" className="tabular-nums">{preferences ? value.toFixed(2) : "—"}</output>
          <button type="button" aria-label={`增大${label}`} title={`增大${label} 0.1`} disabled={!preferences || pending || value >= 2}
            onClick={() => void change({ [key]: Math.round((value + 0.1) * 10) / 10 })}><Plus size={16} /></button>
        </div>
      </div>;
    })}
    <div className="settings-row">
      <span className="setting-label-help"><label htmlFor="island-font-size">文字大小</label><HelpHint label={"文字大小"} text={<>调整标题、字幕与文件名，默认 20；随灵动岛整体比例缩放</>} /></span>
      <Select id="island-font-size" value={String(preferences?.layout.fontSize ?? 20)} disabled={!preferences || pending}
        options={Array.from({ length: 9 }, (_, i) => ({ value: String(i + 16), label: `${i + 16} px` }))}
        onValueChange={value => void change({ fontSize: Number(value) })} />
    </div>
    <div className="settings-row">
      <label htmlFor="island-font-weight">文字粗细</label>
      <Select id="island-font-weight" value={String(preferences?.layout.fontWeight ?? 700)} disabled={!preferences || pending}
        options={[{ value: "400", label: "常规" }, { value: "500", label: "中等" }, { value: "600", label: "半粗" }, { value: "700", label: "粗体（默认）" }, { value: "800", label: "特粗" }]}
        onValueChange={value => void change({ fontWeight: Number(value) })} />
    </div>
    </SettingsDisclosure>
    {error && <ErrorNotice message={error} />}
  </>;
}
