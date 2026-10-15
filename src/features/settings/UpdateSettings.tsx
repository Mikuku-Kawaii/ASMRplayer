import React, { useRef, useState } from "react";
import { RefreshCw } from "lucide-react";
import { AppError, callCommand, normalizeAppError } from "../../lib/api";
import { openReleaseHistory } from "./ReleaseHistory";
import { useUpdater } from "./useUpdater";
import { openUpdateDialog, updateLabels } from "./UpdateDialog";

export const UpdateSettings: React.FC<{ onError: (error: AppError) => void }> = ({ onError }) => {
  const { snapshot: update, run } = useUpdater();
  const [pending, setPending] = useState(false);
  const pendingRef = useRef(false);
  const perform = async (command: string, args?: Record<string, unknown>) => {
    if (pendingRef.current) return;
    pendingRef.current = true; setPending(true);
    try {
      const result = await run(command, args);
      if (command === "check_for_updates" && result?.latestVersion) openUpdateDialog();
    } catch (error) { onError(normalizeAppError(error)); }
    finally { pendingRef.current = false; setPending(false); }
  };
  const busy = !!update && ["checking", "downloading", "verifying", "installing"].includes(update.phase);
  return <section className="settings-card settings-group-card" aria-label="软件更新" data-settings-group="updates">
    <div className="settings-group-header">
      <div className="settings-group-title-wrap">
        <span className="settings-group-icon"><RefreshCw size={16} /></span>
        <div><h3>软件更新</h3><p className="settings-group-desc">从 GitHub 获取正式版本，安装前保存收听进度</p></div>
      </div>
      {update && <span className="badge badge-tech tabular-nums">当前 v{update.currentVersion}</span>}
    </div>
    <div className="settings-group-body">
      <div className="settings-row">
        <div className="settings-label-block"><label htmlFor="update-auto-check">启动时检查更新</label>
          <span className="settings-field-hint">发现新版本时提醒，点击安装后才会退出播放器</span></div>
        <input id="update-auto-check" type="checkbox" role="switch" checked={update?.autoCheck ?? false}
          disabled={!update || pending || busy} onChange={e => void perform("set_update_auto_check", { enabled: e.target.checked })} />
      </div>
      <div className="update-status" role="status">
        <strong>{update ? updateLabels[update.phase] : "打开桌面应用后即可检查更新"}</strong>
        {update?.latestVersion && <span className="tabular-nums">新版本 v{update.latestVersion}</span>}
        {update?.checkedAt && <span className="settings-field-hint">上次检查：{new Date(update.checkedAt).toLocaleString()}</span>}
        {update?.latestVersion && update.skippedVersion === update.latestVersion && <span className="settings-field-hint">已跳过此版本的启动提醒，仍可手动查看</span>}
      </div>
      {update?.error && <div className="error-banner" role="alert">{update.error.message} {update.error.hint}</div>}
      <div className="update-actions">
        <button className="btn btn-secondary" disabled={!update || busy || pending || update.phase === "ready"}
          onClick={() => void perform("check_for_updates")}><RefreshCw size={14} /><span className="ui-icon-label">{update?.phase === "checking" ? "正在检查…" : "检查更新"}</span></button>
        {update?.latestVersion && <button className="btn btn-primary" onClick={openUpdateDialog}>查看更新</button>}
        <button className="btn btn-secondary" disabled={!update} onClick={() => openReleaseHistory("current")}>当前版本说明</button>
        <button className="btn btn-secondary" disabled={!update} onClick={() => openReleaseHistory("history")}>更新历史</button>
        <button className="btn btn-ghost" onClick={() => void callCommand("open_release_page").catch(e => onError(normalizeAppError(e)))}>查看发布页</button>
      </div>
    </div>
  </section>;
};
