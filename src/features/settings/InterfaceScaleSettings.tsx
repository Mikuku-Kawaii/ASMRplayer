import { SettingsSaveNote } from "./SettingsSaveNote";
import { HelpHint } from "../../components/HelpHint";
import { ErrorNotice } from "../../components/ErrorNotice";
import { useCallback, useEffect, useRef, useState } from "react";
import { Minus, Plus } from "lucide-react";
import { callCommand, normalizeAppError } from "../../lib/api";

interface InterfaceScale { scale: number; systemScale: number }

export function InterfaceScaleSettings() {
  const [value, setValue] = useState<InterfaceScale | null>(null);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState("");
  const busy = useRef(false);
  const request = useRef(0);
  const invalidate = useCallback(() => { ++request.current; }, []);
  useEffect(() => {
    const refresh = () => {
      if (busy.current) return;
      const id = ++request.current;
      void callCommand<InterfaceScale>("get_interface_scale").then(next => {
        if (id === request.current) { setValue(next); setError(""); }
      }).catch(e => { if (id === request.current) setError(normalizeAppError(e).message); });
    };
    refresh();
    window.addEventListener("focus", refresh);
    return () => { invalidate(); window.removeEventListener("focus", refresh); };
  }, [invalidate]);
  async function change(scale: number) {
    if (busy.current) return;
    busy.current = true;
    const id = ++request.current;
    setPending(true); setError("");
    try {
      const next = await callCommand<InterfaceScale>("set_interface_scale", { scale: Math.round(scale * 10) / 10 });
      if (id === request.current) setValue(next);
    } catch (e) {
      if (id === request.current) setError(normalizeAppError(e).message);
    } finally {
      busy.current = false;
      if (id === request.current) setPending(false);
    }
  }
  return <>
    <SettingsSaveNote busy={pending} error={error}>自动保存 · 立即调整主窗口缩放</SettingsSaveNote>
    <div className="settings-row">
      <span className="setting-label-help"><span id="interface-scale-label">界面缩放</span><HelpHint label={"界面缩放"} text={<>主窗口 · 每次调整 10%{value && ` · 当前系统缩放 ${Math.round(value.systemScale * 100)}%`}</>} /></span>
      <div className="island-display-actions">
        <div className="scale-stepper" role="group" aria-labelledby="interface-scale-label">
          <button type="button" aria-label="缩小界面" title="缩小界面 10%" disabled={!value || pending || value.scale <= 0.5} onClick={() => void change(value!.scale - 0.1)}><Minus size={16} /></button>
          <output className="tabular-nums" aria-label="界面缩放比例" aria-live="polite">{value ? `${Math.round(value.scale * 100)}%` : "—"}</output>
          <button type="button" aria-label="放大界面" title="放大界面 10%" disabled={!value || pending || value.scale >= 1.5} onClick={() => void change(value!.scale + 0.1)}><Plus size={16} /></button>
        </div>
        <button type="button" className="btn btn-ghost" disabled={!value || pending || value.scale === 1} onClick={() => void change(1)}>恢复 100%</button>
      </div>
    </div>
    {error && <ErrorNotice message={error} />}
  </>;
}
