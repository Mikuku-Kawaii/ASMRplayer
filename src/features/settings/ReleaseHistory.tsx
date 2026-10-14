import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { History, RefreshCw, X } from "lucide-react";
import { Select } from "../../components/Select";
import { callCommand, isTauriRuntime, normalizeAppError, type AppError } from "../../lib/api";
import { ReleaseNotes } from "./ReleaseNotes";

export interface ReleaseEntry { version: string; title: string; notes: string | null; publishedAt: string | null; url: string }
export interface ReleaseHistorySnapshot { currentVersion: string; entries: ReleaseEntry[]; fetchedAt: string | null; pendingVersion: string | null; error: AppError | null }
type View = "current" | "history";
export function openReleaseHistory(view: View = "history") { window.dispatchEvent(new CustomEvent("updater:history", { detail: view })); }
const date = (value: string) => { const parsed = new Date(value); return Number.isNaN(parsed.getTime()) ? "日期未知" : parsed.toLocaleString(); };

/** Mounted once in the main navigation; no network requests or modal on startup. */
export function ReleaseHistoryHost({ enabled = true, onOpenChange }: { enabled?: boolean; onOpenChange?: (open: boolean) => void }) {
  const [view, setView] = useState<View | null>(null);
  const [pendingVersion, setPendingVersion] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [visible, setVisible] = useState(false);
  const claiming = useRef(false), alive = useRef(true);
  useEffect(() => {
    alive.current = true;
    let disposed = false;
    let stop: (() => void) | undefined;
    const refreshVisibility = async () => {
      const shown = document.visibilityState !== "hidden" && (!isTauriRuntime() ||
        (await getCurrentWindow().isVisible() && !await getCurrentWindow().isMinimized()));
      if (!disposed) setVisible(shown);
    };
    const refresh = () => { void refreshVisibility().catch(() => {}); };
    const open = (event: Event) => { setView((event as CustomEvent<View>).detail === "current" ? "current" : "history"); };
    void callCommand<ReleaseHistorySnapshot>("get_release_history").then(next => { if (!disposed) setPendingVersion(next?.pendingVersion ?? null); }).catch(() => {});
    window.addEventListener("updater:history", open);
    window.addEventListener("focus", refresh);
    document.addEventListener("visibilitychange", refresh);
    if (isTauriRuntime()) void getCurrentWindow().onFocusChanged(refresh).then(fn => { if (disposed) fn(); else stop = fn; }).catch(() => {});
    refresh();
    return () => {
      disposed = true; alive.current = false; stop?.();
      window.removeEventListener("updater:history", open);
      window.removeEventListener("focus", refresh);
      document.removeEventListener("visibilitychange", refresh);
    };
  }, []);
  useEffect(() => {
    if (!enabled || !visible || !pendingVersion || claiming.current) return;
    claiming.current = true;
    void callCommand<boolean>("claim_upgrade_notice", { version: pendingVersion }).then(claimed => {
      if (alive.current) { if (claimed) setNotice(pendingVersion); setPendingVersion(null); }
    }).catch(() => { /* A later visibility change retries; manual history remains available. */ })
      .finally(() => { claiming.current = false; });
  }, [enabled, visible, pendingVersion]);
  const open = !!view && enabled;
  useEffect(() => { onOpenChange?.(open); return () => onOpenChange?.(false); }, [open, onOpenChange]);
  return <>
    {enabled && notice && <aside className="upgrade-notice" aria-label="升级完成说明">
      <div><History size={16} /><span role="status">已升级至 v{notice}</span><button type="button" className="btn-icon" aria-label="关闭升级提示" title="关闭升级提示" onClick={() => setNotice(null)}><X size={14} /></button></div>
      <p>查看新增功能与设置变化</p>
      <button type="button" className="btn btn-ghost btn-sm" onClick={() => { setView("current"); setNotice(null); }}>查看升级说明</button>
    </aside>}
    {open && <ReleaseHistoryDialog initialView={view!} onClose={() => setView(null)} />}
  </>;
}

export function ReleaseHistoryDialog({ initialView, onClose }: { initialView: View; onClose: () => void }) {
  const dialog = useRef<HTMLDialogElement>(null), closeButton = useRef<HTMLButtonElement>(null);
  const [history, setHistory] = useState<ReleaseHistorySnapshot | null>(null);
  const [selected, setSelected] = useState("");
  const [busy, setBusy] = useState(true);
  const [error, setError] = useState<AppError | null>(null);
  const locked = useRef(false), mounted = useRef(true), request = useRef(0);
  useEffect(() => {
    mounted.current = true;
    const el = dialog.current, previous = document.activeElement as HTMLElement | null;
    const main = document.querySelector<HTMLElement>(".main-content"), overflow = main?.style.overflow ?? "";
    el?.showModal(); closeButton.current?.focus();
    if (main) main.style.overflow = "hidden";
    const id = ++request.current;
    void callCommand<ReleaseHistorySnapshot>("get_release_history").then(next => {
      if (!mounted.current || request.current !== id) return;
      setHistory(next); setError(next.error);
      setSelected(initialView === "current" ? next.currentVersion : next.entries[0]?.version ?? next.currentVersion);
    }).catch(e => { if (mounted.current && request.current === id) setError(normalizeAppError(e)); })
      .finally(() => { if (mounted.current && request.current === id) setBusy(false); });
    return () => {
      mounted.current = false; el?.close();
      if (main) main.style.overflow = overflow;
      if (previous?.isConnected) previous.focus({ preventScroll: true });
    };
  }, [initialView]);
  async function refresh() {
    if (locked.current) return;
    locked.current = true; const id = ++request.current; setBusy(true); setError(null);
    try {
      const next = await callCommand<ReleaseHistorySnapshot>("refresh_release_history");
      if (mounted.current && request.current === id) {
        setHistory(next); setError(next.error);
        setSelected(current => current || (initialView === "current" ? next.currentVersion : next.entries[0]?.version ?? next.currentVersion));
      }
    } catch (e) { if (mounted.current && request.current === id) setError(normalizeAppError(e)); }
    finally { locked.current = false; if (mounted.current && request.current === id) setBusy(false); }
  }
  const entry = history?.entries.find(item => item.version === selected);
  const entries = history ? history.entries.some(item => item.version === history.currentVersion)
    ? history.entries : [{ version: history.currentVersion, title: "", notes: null, publishedAt: null, url: "" }, ...history.entries] : [];
  return createPortal(<dialog ref={dialog} className="modal-card storage-dialog release-history-dialog" aria-modal="true" aria-labelledby="release-history-title"
    onCancel={e => { e.preventDefault(); onClose(); }} onClick={e => { if (e.target === e.currentTarget) { const box = e.currentTarget.getBoundingClientRect(); if (e.clientX < box.left || e.clientX > box.right || e.clientY < box.top || e.clientY > box.bottom) onClose(); } }}
    onKeyDown={e => {
      e.stopPropagation();
      if (e.key !== "Tab") return;
      const controls = Array.from(dialog.current?.querySelectorAll<HTMLElement>('button:not(:disabled),a[href],[tabindex="0"]') ?? []);
      if (e.shiftKey && document.activeElement === controls[0]) { e.preventDefault(); controls.at(-1)?.focus(); }
      else if (!e.shiftKey && document.activeElement === controls.at(-1)) { e.preventDefault(); controls[0]?.focus(); }
    }}>
    <header className="storage-heading"><div><h2 id="release-history-title">更新历史</h2><p className="settings-field-hint">{history ? `当前安装 v${history.currentVersion} · 新增功能与设置变化` : "读取本机发布说明…"}</p></div>
      <button ref={closeButton} type="button" className="btn-icon" aria-label="关闭更新历史" title="关闭更新历史 (Esc)" onClick={onClose}><X size={18} /></button></header>
    <div className="release-history-toolbar"><label htmlFor="release-history-version">发布版本</label><Select id="release-history-version" disabled={!history} value={selected} options={entries.map(item => ({ value: item.version, label: `v${item.version}${item.version === history?.currentVersion ? " · 当前版本" : ""}${item.title && item.title !== `v${item.version}` ? ` · ${item.title}` : ""}` }))} onValueChange={setSelected} />
      <button type="button" className="btn btn-secondary" disabled={busy} onClick={() => void refresh()}><RefreshCw size={14} />{busy ? "正在读取…" : "刷新历史"}</button></div>
    <p className="settings-field-hint" role="status">{history?.fetchedAt ? `本机缓存 · 最近刷新 ${date(history.fetchedAt)}` : "打开时优先读取本机缓存；点击刷新历史获取最近 30 条发布记录中的正式版本。"}</p>
    {error && <div role="alert" className="error-banner">{error.message} {error.hint}</div>}
    <section className="release-history-body" tabIndex={0} aria-label="历史发布说明">
      {entry ? <><h3>{entry.title || `v${entry.version}`}</h3>{entry.publishedAt && <p className="settings-field-hint">发布于 {date(entry.publishedAt)}</p>}<ReleaseNotes notes={entry.notes} onError={setError} /></>
        : <p className="settings-field-hint">{busy ? "正在读取发布说明…" : "此版本尚无本机缓存。可刷新历史获取说明，或打开发布页查看。"}</p>}
    </section>
    <footer className="update-actions"><button type="button" className="btn btn-ghost" onClick={() => void callCommand(entry ? "open_update_link" : "open_release_page", entry ? { url: entry.url } : undefined).catch(e => setError(normalizeAppError(e)))}>查看发布页</button><button type="button" className="btn btn-primary" onClick={onClose}>关闭说明</button></footer>
  </dialog>, document.body);
}
