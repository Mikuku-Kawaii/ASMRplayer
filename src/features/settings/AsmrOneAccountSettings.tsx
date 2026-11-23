import { useState } from "react";
import { UserRound } from "lucide-react";
import { callCommand, normalizeAppError, type AppSettingsDto } from "../../lib/api";
import { AsmrOneLibraryDialog } from "./AsmrOneLibraryDialog";

export const asmrOneMirrors = ["api.asmr-200.com", "api.asmr.one", "api.asmr-100.com", "api.asmr-300.com"];

function importAccountKey(settings: AppSettingsDto) {
  return settings.hasAuthToken ? JSON.stringify([settings.apiBaseUrl, settings.siteAccountName ?? null]) : "";
}

export function AsmrOneAccountSettings({ settings, onChange, chooseMirror }: {
  settings: AppSettingsDto; onChange: (value: AppSettingsDto) => void; chooseMirror: () => void;
}) {
  const [name, setName] = useState("");
  const [password, setPassword] = useState("");
  const [editing, setEditing] = useState(false);
  const [pending, setPending] = useState<"login" | "logout" | null>(null);
  const [error, setError] = useState("");
  const [importFor, setImportFor] = useState<string | null>(null);
  const [importResult, setImportResult] = useState<{ account: string; added: number; lists?: number } | null>(null);
  const account = settings.siteAccountName;
  const accountKey = importAccountKey(settings);
  const allowed = asmrOneMirrors.some(host => settings.apiBaseUrl === `https://${host}`);
  const canSubmit = allowed && !pending && !!name.trim() && !!password;

  async function submit(action: "login" | "logout") {
    if (pending || (action === "login" && !canSubmit)) return;
    setPending(action);
    setError("");
    try {
      const apiBase = settings.apiBaseUrl;
      const next = action === "login"
        ? await callCommand<AppSettingsDto>("login_asmr_one", { apiBase, name: name.trim(), password })
        : await callCommand<AppSettingsDto>("logout_asmr_one", { apiBase });
      onChange(next);
      setPassword("");
      setName("");
      setEditing(false);
      setImportResult(null);
      setImportFor(action === "login" ? importAccountKey(next) : null);
    } catch (failure) {
      setError(normalizeAppError(failure).message);
    } finally { setPending(null); }
  }

  return <>
    <div className="settings-group-header">
      <div className="settings-group-title-wrap">
        <span className="settings-group-icon"><UserRound size={16} aria-hidden="true" /></span><h3>asmr.one 账号</h3>
      </div>
      {!account && <span className="settings-field-hint">{settings.hasAuthToken ? "已设置令牌" : "未登录"}</span>}
    </div>
    <div className="settings-group-body">
    {account && !editing ? <div className="settings-row">
      <label htmlFor="setting-site-account">站点账号</label>
      <div className="input-with-btn">
        <span>已连接账号 · {account}</span>
        <button id="setting-site-account" type="button" aria-label="重新登录 asmr.one" className="btn btn-secondary btn-sm" disabled={!!pending}
          onClick={() => { setName(account); setError(""); setEditing(true); }}>重新登录</button>
        <button type="button" className="btn btn-ghost btn-sm" disabled={!!pending} onClick={() => void submit("logout")}>
          {pending === "logout" ? "正在退出…" : "退出登录"}</button>
      </div>
    </div> : allowed ? <form className="asmr-one-login" aria-label="asmr.one 账号登录" onSubmit={event => { event.preventDefault(); void submit("login"); }}>
      <div className="settings-row"><label htmlFor="setting-site-account">账号名</label>
        <input id="setting-site-account" className="input-text" name="username" autoComplete="username" autoCapitalize="none"
          spellCheck={false} required disabled={!!pending} value={name} onChange={e => setName(e.target.value)} /></div>
      <div className="settings-row"><label htmlFor="setting-site-password">密码</label>
        <input id="setting-site-password" className="input-text" type="password" name="password" autoComplete="current-password"
          required disabled={!!pending} value={password} onChange={e => setPassword(e.target.value)} /></div>
      <div className="input-with-btn">
        <button type="submit" className="btn btn-primary btn-sm" disabled={!canSubmit}>{pending === "login" ? "正在登录…" : "登录 asmr.one"}</button>
        {editing && <button type="button" className="btn btn-ghost btn-sm" disabled={!!pending}
          onClick={() => { setPassword(""); setEditing(false); setError(""); }}>取消</button>}
      </div>
    </form> : <div className="settings-row">
      <label htmlFor="setting-site-account">选择 asmr.one 官方镜像即可登录。</label>
      <button id="setting-site-account" type="button" aria-label="选择官方镜像" className="btn btn-secondary btn-sm" onClick={chooseMirror}>选择官方镜像</button>
    </div>}
    {settings.hasAuthToken && !editing && <div className="settings-row">
      <span>音频库</span>
      <div className="input-with-btn"><button type="button" className="btn btn-secondary btn-sm" id="setting-site-library-import"
        disabled={!!pending} onClick={() => { setImportResult(null); setImportFor(accountKey); }}>同步收藏与播放列表</button>
        {importResult?.account === accountKey && <span role="status">已加入 {importResult.added} 部作品。{importResult.lists ? `已保存 ${importResult.lists} 个作品集。` : ""}</span>}
      </div>
    </div>}
    {accountKey && importFor === accountKey && <AsmrOneLibraryDialog close={(added, lists) => {
      setImportFor(null);
      if (added !== undefined) setImportResult({ account: accountKey, added, lists });
    }} />}
    {error && <p role="alert" className="asmr-one-login-error">{error}</p>}
    </div>
  </>;
}
