import { useEffect, useRef, useState } from "react";
import { callCommand, normalizeAppError } from "../../lib/api";
import { ErrorNotice } from "../../components/ErrorNotice";
import { SettingsSaveNote } from "./SettingsSaveNote";
import { normalizeFilePreferences, type FilePreferences } from "../library/filePreferences";

export function FilePreferencesSettings() {
  const [value, setValue] = useState<FilePreferences | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [attempt, setAttempt] = useState(0);
  const gate = useRef(false);
  useEffect(() => {
    let disposed = false;
    void callCommand<FilePreferences>("get_file_preferences").then(next => {
      if (!disposed) { setValue(normalizeFilePreferences(next)); setError(""); }
    }).catch(reason => { if (!disposed) setError(normalizeAppError(reason).message); });
    return () => { disposed = true; };
  }, [attempt]);
  async function change(next: FilePreferences) {
    if (gate.current) return;
    gate.current = true; setBusy(true); setError("");
    try { setValue(await callCommand<FilePreferences>("set_file_preferences", { preferences: next })); }
    catch (reason) { setError(normalizeAppError(reason).message); }
    finally { gate.current = false; setBusy(false); }
  }
  return <section className="settings-card settings-group-card" aria-labelledby="file-preferences-title">
    <div className="settings-group-header"><h3 id="file-preferences-title">文件偏好</h3></div>
    <div className="settings-group-body">
      <SettingsSaveNote busy={busy} error={error}>自动保存 · 本地与在线作品优先展示匹配目录</SettingsSaveNote>
      <fieldset className="file-preference-options" disabled={!value || busy} aria-describedby="file-effects-help">
        <legend data-settings-search="文件偏好 效果音版本 SE 有效果音 无效果音">效果音版本</legend>
        <div>{([['any', '不指定'], ['with', '有效果音'], ['without', '无效果音']] as const).map(([key, label]) =>
          <label key={key} htmlFor={`file-effects-${key}`}><input id={`file-effects-${key}`} type="radio" name="file-effects" value={key}
            checked={value?.effects === key} onChange={() => value && void change({ ...value, effects: key })} /><span>{label}</span></label>)}</div>
        <p id="file-effects-help" className="settings-field-hint">根据目录名称识别 SEあり、SEなし、含音效、無SE音效等版本，不改变音频本身。</p>
      </fieldset>
      <fieldset className="file-preference-options" disabled={!value || busy} aria-describedby="file-format-help">
        <legend data-settings-search="文件偏好 音频格式 MP3 WAV">音频格式</legend>
        <div>{([['any', '不指定'], ['mp3', 'MP3'], ['wav', 'WAV']] as const).map(([key, label]) =>
          <label key={key} htmlFor={`file-format-${key}`}><input id={`file-format-${key}`} type="radio" name="file-format" value={key}
            checked={value?.format === key} onChange={() => value && void change({ ...value, format: key })} /><span>{label}</span></label>)}</div>
        <p id="file-format-help" className="settings-field-hint">正片优先于试听与附赠内容，再匹配效果音和格式。同等匹配优先上次未听完的目录；仍可手动切换其他版本。</p>
      </fieldset>
      {error && <ErrorNotice message={error} />}
      {!value && error && <button type="button" className="btn btn-secondary" onClick={() => setAttempt(previous => previous + 1)}>重新读取</button>}
    </div>
  </section>;
}
