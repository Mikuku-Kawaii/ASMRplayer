import React, { useRef, useState } from "react";
import { Download, RefreshCw } from "lucide-react";
import { AppError, callCommand, formatBytes, normalizeAppError } from "../../lib/api";
import { useUpdater } from "./useUpdater";

export const UpdateSettings: React.FC<{ onError: (error: AppError) => void }> = ({ onError }) => {
  const { snapshot: update, run } = useUpdater();
  const [pending, setPending] = useState(false);
  const pendingRef = useRef(false);
  const perform = async (command: string, args?: Record<string, unknown>) => {
    if (pendingRef.current) return;
    pendingRef.current = true; setPending(true);
    try { await run(command, args); }
    catch (error) { onError(normalizeAppError(error)); }
    finally { pendingRef.current = false; setPending(false); }
  };
  const busy = !!update && ["checking", "downloading", "verifying", "installing"].includes(update.phase);
  const labels = {
    idle: "尚未检查更新", checking: "正在检查更新…", up_to_date: "当前已是最新正式版",
    available: "发现新的正式版本", downloading: "正在下载更新…", verifying: "正在验证更新签名…",
    ready: "更新包已验证，可安装", installing: "正在保存收听状态并安装…", error: "更新未完成",
  };
  return (
    <section className="settings-card settings-group-card" aria-label="软件更新" data-settings-group="updates">
      <div className="settings-group-header">
        <div className="settings-group-title-wrap">
          <span className="settings-group-icon"><RefreshCw size={16} /></span>
          <div><h3>软件更新</h3><p className="settings-group-desc">从 GitHub 获取正式版本，安装前保存收听进度</p></div>
        </div>
        {update && <span className="badge badge-tech tabular-nums">当前 v{update.currentVersion}</span>}
      </div>
      <div className="settings-group-body">
        <div className="settings-row">
          <div className="settings-label-block">
            <label htmlFor="update-auto-check">启动时检查更新</label>
            <span className="settings-field-hint">发现新版本时提醒，点击安装后才会退出播放器</span>
          </div>
          <input id="update-auto-check" type="checkbox" role="switch" checked={update?.autoCheck ?? false}
            disabled={!update || pending || busy} onChange={(e) => void perform("set_update_auto_check", { enabled: e.target.checked })} />
        </div>
        <div className="update-status" role="status" aria-live="polite">
          <strong>{update ? labels[update.phase] : "打开桌面应用后即可检查更新"}</strong>
          {update?.latestVersion && <span className="tabular-nums">新版本 v{update.latestVersion}</span>}
          {update?.checkedAt && <span className="settings-field-hint">上次检查：{new Date(update.checkedAt).toLocaleString()}</span>}
          {update?.skippedVersion === update?.latestVersion && update?.latestVersion && <span className="settings-field-hint">已跳过此版本的启动提醒，仍可手动下载</span>}
        </div>
        {update?.error && <div className="error-banner" role="alert">{update.error.message}<br />{update.error.hint}</div>}
        {update && ["downloading", "verifying", "ready"].includes(update.phase) && (
          <div className="update-progress">
            <progress aria-label="更新下载进度" max={update.totalBytes || undefined} value={update.totalBytes ? Math.min(update.downloadedBytes, update.totalBytes) : undefined} />
            <span className="tabular-nums">{formatBytes(update.downloadedBytes)}{update.totalBytes ? ` / ${formatBytes(update.totalBytes)}` : " · 总大小未知"}</span>
          </div>
        )}
        {update?.notes && <details className="update-notes" open><summary>更新内容</summary><p>{update.notes}</p></details>}
        {update?.phase === "ready" && <p className="settings-field-hint">安装会暂停播放并重启应用；音频库、书签和离线文件会保留。</p>}
        <div className="update-actions">
          <button type="button" className="btn btn-secondary" disabled={!update || busy || pending || update.phase === "ready"}
            onClick={() => void perform("check_for_updates")}><RefreshCw size={14} />{update?.phase === "checking" ? "正在检查…" : "检查更新"}</button>
          {update?.latestVersion && ["available", "error"].includes(update.phase) && <button type="button" className="btn btn-primary" disabled={pending}
            onClick={() => void perform("download_update")}><Download size={14} />下载更新</button>}
          {update?.phase === "ready" && <button type="button" className="btn btn-primary" disabled={pending}
            onClick={() => void perform("install_update")}>安装并重启</button>}
          {update?.phase === "downloading" && <button type="button" className="btn btn-ghost"
            onClick={() => void run("cancel_update_download").catch((error) => onError(normalizeAppError(error)))}>取消下载</button>}
          {update?.latestVersion && !busy && <>
            <button type="button" className="btn btn-ghost" disabled={pending || update.dismissed} onClick={() => void perform("dismiss_update", { skip: false })}>稍后提醒</button>
            <button type="button" className="btn btn-ghost" disabled={pending || update.skippedVersion === update.latestVersion} onClick={() => void perform("dismiss_update", { skip: true })}>跳过此版本</button>
          </>}
          <button type="button" className="btn btn-ghost" onClick={() => void callCommand("open_release_page").catch((error) => onError(normalizeAppError(error)))}>打开发布页</button>
        </div>
      </div>
    </section>
  );
};
