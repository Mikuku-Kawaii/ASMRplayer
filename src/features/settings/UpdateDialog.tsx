import { useCallback, useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { X } from "lucide-react";
import { AppError, callCommand, formatBytes, isTauriRuntime, normalizeAppError, UpdateSnapshot } from "../../lib/api";
import { ReleaseNotes } from "./ReleaseNotes";

export const updateLabels: Record<UpdateSnapshot["phase"], string> = {
  idle: "尚未检查更新", checking: "正在检查更新…", up_to_date: "当前已是最新正式版",
  available: "发现新的正式版本", downloading: "正在下载更新…", verifying: "正在验证更新签名…",
  ready: "更新包已验证，可安装", installing: "正在保存收听状态并安装…", error: "更新未完成",
};
export function openUpdateDialog() { window.dispatchEvent(new Event("updater:open")); }

export function UpdateDialogHost({ update, run, enabled = true }: {
  update: UpdateSnapshot | null;
  run: (command: string, args?: Record<string, unknown>) => Promise<unknown>;
  enabled?: boolean;
}) {
  const [open, setOpen] = useState(false);
  const [visible, setVisible] = useState(false);
  const seen = useRef(new Set<string>());
  const manual = useRef(false);
  useEffect(() => {
    let disposed = false;
    let stop: (() => void) | undefined;
    const refresh = async () => {
      const shown = isTauriRuntime()
        ? await getCurrentWindow().isVisible() && !await getCurrentWindow().isMinimized()
        : document.visibilityState !== "hidden";
      if (!disposed) setVisible(shown);
    };
    const refreshQuietly = () => { void refresh().catch(() => {}); };
    const requestOpen = () => { manual.current = true; refreshQuietly(); setOpen(true); };
    window.addEventListener("updater:open", requestOpen);
    window.addEventListener("focus", refreshQuietly);
    document.addEventListener("visibilitychange", refreshQuietly);
    if (isTauriRuntime()) void getCurrentWindow().onFocusChanged(refreshQuietly).then(fn => { if (disposed) fn(); else stop = fn; }).catch(() => {});
    refreshQuietly();
    return () => {
      disposed = true; stop?.();
      window.removeEventListener("updater:open", requestOpen);
      window.removeEventListener("focus", refreshQuietly);
      document.removeEventListener("visibilitychange", refreshQuietly);
    };
  }, []);
  useEffect(() => {
    if (!visible || !enabled || !update?.latestVersion) return;
    let disposed = false;
    // Recheck native visibility at reminder time; a hide can race with metadata.
    void (async () => {
      if (isTauriRuntime() && (!await getCurrentWindow().isVisible() || await getCurrentWindow().isMinimized())) {
        if (!disposed) setVisible(false);
        return;
      }
      if (!disposed && (manual.current || (!update.dismissed && update.skippedVersion !== update.latestVersion &&
        ["available", "ready"].includes(update.phase) && !seen.current.has(update.latestVersion!)))) {
        seen.current.add(update.latestVersion!); manual.current = false; setOpen(true);
      }
    })().catch(() => {});
    return () => { disposed = true; };
  }, [update, visible, enabled]);
  return open && visible && enabled && update?.latestVersion ? <UpdateDialog update={update} run={run} onClose={() => {
    seen.current.add(update.latestVersion!); manual.current = false; setOpen(false);
  }} /> : null;
}

export function UpdateDialog({ update, run, onClose }: {
  update: UpdateSnapshot; run: (command: string, args?: Record<string, unknown>) => Promise<unknown>; onClose: () => void;
}) {
  const dialog = useRef<HTMLDivElement>(null);
  const [error, setError] = useState<AppError | null>(null);
  const [pending, setPending] = useState<string | null>(null);
  const pendingRef = useRef<string | null>(null);
  const closeRef = useRef<() => void>(() => {});
  const locked = update.phase === "installing" || pending === "install_update";
  const perform = useCallback(async (command: string, args?: Record<string, unknown>) => {
    if (pendingRef.current) return;
    pendingRef.current = command; setPending(command); setError(null);
    try { await run(command, args); }
    catch (e) { setError(normalizeAppError(e)); }
    finally { pendingRef.current = null; setPending(null); }
  }, [run]);
  const close = () => {
    if (locked) return;
    // Closing is independent of the download command's lifetime.
    void run("dismiss_update", { skip: false }).catch(() => {});
    onClose();
  };
  closeRef.current = close;
  useEffect(() => {
    const previous = document.activeElement as HTMLElement | null;
    const shell = document.querySelector<HTMLElement>(".app-shell");
    const previousInert = shell?.inert;
    if (shell) shell.inert = true;
    const overflow = document.body.style.overflow;
    document.body.style.overflow = "hidden";
    document.body.classList.add("update-dialog-open");
    dialog.current?.focus();
    const keydown = (event: KeyboardEvent) => {
      // A nested Select owns selection/Escape; Tab still follows this dialog's order.
      const select = event.target instanceof Element ? event.target.closest('[data-select-control]') : null;
      if (select && event.key !== "Tab" && (event.key !== "Escape" || select.getAttribute("aria-expanded") === "true")) return;
      // Capture before the app's playback shortcuts, including events from the background.
      if ([" ", "ArrowLeft", "ArrowRight", "Escape", "Tab"].includes(event.key)) event.stopPropagation();
      if (event.key === "Escape") { event.preventDefault(); closeRef.current(); }
      if (event.key === "Tab") {
        const elements = Array.from(dialog.current?.querySelectorAll<HTMLElement>('button:not(:disabled), a[href], [tabindex="0"]') ?? []);
        const index = elements.indexOf(document.activeElement as HTMLElement);
        const next = event.shiftKey ? (index <= 0 ? elements.length - 1 : index - 1) : (index + 1) % elements.length;
        event.preventDefault(); (elements[next] ?? dialog.current)?.focus();
      }
    };
    const focus = (event: FocusEvent) => { if (!dialog.current?.contains(event.target as Node)) dialog.current?.focus(); };
    document.addEventListener("keydown", keydown, true);
    document.addEventListener("focusin", focus);
    return () => {
      document.removeEventListener("keydown", keydown, true); document.removeEventListener("focusin", focus);
      document.body.style.overflow = overflow; document.body.classList.remove("update-dialog-open");
      if (shell) shell.inert = previousInert ?? false;
      if (previous?.isConnected) previous.focus();
    };
  }, []);
  const actualError = error ?? update.error;
  return createPortal(<div className="update-dialog-backdrop" onClick={e => { if (e.target === e.currentTarget) close(); }}>
    <div className="update-dialog modal-card" role="dialog" aria-modal="true" aria-labelledby="update-title" tabIndex={-1} ref={dialog}>
      <header className="modal-header"><div><h2 id="update-title">软件更新</h2><p className="tabular-nums">v{update.currentVersion} → v{update.latestVersion}</p>
        {update.publishedAt && <p className="settings-field-hint">发布日期：{new Date(update.publishedAt).toLocaleDateString()}</p>}</div>
        {!locked && <button className="btn-icon" aria-label="稍后提醒" title="稍后提醒 (Esc)" onClick={close}><X size={18} /></button>}
      </header>
      <div className="update-dialog-body" tabIndex={0} role="region" aria-label="更新内容">
        <ReleaseNotes notes={update.notes} onError={setError} />
      </div>
      <div className="update-dialog-status">
        <p role="status" aria-live="polite">{updateLabels[update.phase]}</p>
        {["downloading", "verifying", "ready"].includes(update.phase) && <div className="update-progress">
          <progress aria-label="更新下载进度" max={update.totalBytes || undefined} value={update.totalBytes ? Math.min(update.downloadedBytes, update.totalBytes) : undefined} />
          <span className="tabular-nums">{formatBytes(update.downloadedBytes)}{update.totalBytes ? ` / ${formatBytes(update.totalBytes)}` : " · 总大小未知"}</span>
        </div>}
        {actualError && <div role="alert" className="error-banner">{actualError.message} {actualError.hint}</div>}
        {update.phase === "ready" && <p className="settings-field-hint">安装会暂停播放并重启应用；音频库、收藏、书签和离线文件会保留。</p>}
      </div>
      <footer className="update-actions">
        <button className="btn btn-ghost" disabled={locked} onClick={() => void callCommand("open_release_page").catch(e => setError(normalizeAppError(e)))}>查看发布页</button>
        {!locked && <>
          <button className="btn btn-ghost" onClick={close}>稍后提醒</button>
          <button className="btn btn-ghost" disabled={!!pending || ["downloading", "verifying"].includes(update.phase) || update.skippedVersion === update.latestVersion}
            onClick={() => { if (!pendingRef.current) void run("dismiss_update", { skip: true }).then(onClose).catch(e => setError(normalizeAppError(e))); }}>跳过此版本</button>
        </>}
        {["available", "error"].includes(update.phase) && <button className="btn btn-primary" disabled={!!pending} onClick={() => void perform("download_update")}>{update.phase === "error" ? "重试下载" : "下载更新"}</button>}
        {update.phase === "downloading" && <button className="btn btn-secondary" onClick={() => void run("cancel_update_download").catch(e => setError(normalizeAppError(e)))}>取消下载</button>}
        {(update.phase === "ready" || locked) && <button className="btn btn-primary" disabled={!!pending || locked} onClick={() => void perform("install_update")}>{locked ? "正在安装…" : "安装并重启"}</button>}
      </footer>
    </div>
  </div>, document.body);
}
