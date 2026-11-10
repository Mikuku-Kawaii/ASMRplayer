import { SettingsSaveNote } from "./SettingsSaveNote";
import { useEffect, useState } from "react";
import { callCommand, normalizeAppError } from "../../lib/api";
import { ErrorNotice } from "../../components/ErrorNotice";

interface MaintenanceSettings { idleMinutes: number | null; completedTracks: number | null }

export function EngineMaintenanceSettings() {
  const [loaded, setLoaded] = useState(false);
  const [idleEnabled, setIdleEnabled] = useState(false);
  const [tracksEnabled, setTracksEnabled] = useState(false);
  const [minutes, setMinutes] = useState(60);
  const [tracks, setTracks] = useState(10);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [saved, setSaved] = useState(false);
  const [baseline, setBaseline] = useState("");
  useEffect(() => {
    let live = true;
    void callCommand<MaintenanceSettings>("get_engine_maintenance").then(value => {
      if (!live || !value) return;
      setIdleEnabled(value.idleMinutes !== null); setTracksEnabled(value.completedTracks !== null);
      setBaseline(JSON.stringify(value));
      setMinutes(value.idleMinutes ?? 60); setTracks(value.completedTracks ?? 10); setLoaded(true);
    }).catch(e => { if (live) setError(normalizeAppError(e).message); });
    return () => { live = false; };
  }, []);
  const current = { idleMinutes: idleEnabled ? minutes : null, completedTracks: tracksEnabled ? tracks : null };
  const dirty = loaded && JSON.stringify(current) !== baseline;
  const valid = (!idleEnabled || (Number.isInteger(minutes) && minutes >= 1 && minutes <= 1440))
    && (!tracksEnabled || (Number.isInteger(tracks) && tracks >= 1 && tracks <= 1000));
  async function save() {
    if (busy || !valid) return;
    setBusy(true); setError(""); setSaved(false);
    try {
      await callCommand<MaintenanceSettings>("set_engine_maintenance", { settings: {
        idleMinutes: idleEnabled ? minutes : null, completedTracks: tracksEnabled ? tracks : null,
      } });
      setBaseline(JSON.stringify(current)); setSaved(true);
    } catch (e) { setError(normalizeAppError(e).message); }
    finally { setBusy(false); }
  }
  return <div className="engine-maintenance-settings" aria-label="定期重启引擎">
    <SettingsSaveNote busy={busy} error={error} dirty={dirty}>手动保存 · 保存后重新计时、计数，不立即重启引擎</SettingsSaveNote>
    <div className="settings-row">
      <label htmlFor="engine-idle-minutes">未播放时重启</label>
      <div className="island-display-actions">
        <button type="button" className={`dsp-scope-toggle-btn ${idleEnabled ? "is-lit" : "is-dim"}`} aria-pressed={idleEnabled}
          disabled={!loaded || busy} onClick={() => { setIdleEnabled(v => !v); setSaved(false); }}>定时</button>
        <input id="engine-idle-minutes" className="input-text" type="number" min={1} max={1440} step={1} value={minutes}
          disabled={!loaded || busy || !idleEnabled} onChange={e => { setMinutes(Number(e.target.value)); setSaved(false); }} />
        <span>分钟</span>
      </div>
    </div>
    <div className="settings-row">
      <label htmlFor="engine-track-count">播完音轨后重启</label>
      <div className="island-display-actions">
        <button type="button" className={`dsp-scope-toggle-btn ${tracksEnabled ? "is-lit" : "is-dim"}`} aria-pressed={tracksEnabled}
          disabled={!loaded || busy} onClick={() => { setTracksEnabled(v => !v); setSaved(false); }}>按音轨数</button>
        <input id="engine-track-count" className="input-text" type="number" min={1} max={1000} step={1} value={tracks}
          disabled={!loaded || busy || !tracksEnabled} onChange={e => { setTracks(Number(e.target.value)); setSaved(false); }} />
        <span>首</span>
      </div>
    </div>
    <p className="settings-desc">暂停或空闲时计时；自然播完后计数，在下一轨开始前重启。保存或重启后重新计时、计数。</p>
    <div className="settings-actions">
      <button type="button" className="btn btn-secondary" disabled={!loaded || busy || !valid || !dirty} onClick={() => void save()}>{busy ? "保存中…" : "保存重启设置"}</button>
      {saved && <span role="status">已保存</span>}
    </div>
    <ErrorNotice message={error} />
  </div>;
}
