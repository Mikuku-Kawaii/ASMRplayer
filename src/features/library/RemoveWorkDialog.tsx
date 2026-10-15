import { useEffect, useId, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { Trash2, X } from "lucide-react";
import { normalizeAppError, WorkSummary } from "../../lib/api";

export const SKIP_LIBRARY_REMOVAL_KEY = "asmr-player.skip-library-index-removal-confirmation";
export function skipsLibraryRemovalConfirmation() {
  try { return localStorage.getItem(SKIP_LIBRARY_REMOVAL_KEY) === "true"; } catch { return false; }
}

export function RemoveWorkDialog({ work, onConfirm, onClose }: {
  work: WorkSummary; onConfirm: (deleteDownloadedFiles: boolean) => Promise<void>; onClose: () => void;
}) {
  const titleId = useId(), descriptionId = useId();
  const dialog = useRef<HTMLDivElement>(null);
  const cancel = useRef<HTMLButtonElement>(null);
  const [deleteFiles, setDeleteFiles] = useState(false);
  const [skipNext, setSkipNext] = useState(false);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState("");
  const busy = useRef(false);
  const close = useRef(onClose);
  close.current = onClose;
  useEffect(() => {
    const previous = document.activeElement as HTMLElement | null;
    const shell = document.querySelector<HTMLElement>(".app-shell");
    const inert = shell?.inert;
    const main = document.querySelector<HTMLElement>(".main-content");
    const overflow = main?.style.overflow ?? "";
    const bodyOverflow = document.body.style.overflow;
    if (shell) shell.inert = true;
    if (main) main.style.overflow = "hidden";
    document.body.style.overflow = "hidden";
    cancel.current?.focus();
    const focus = (event: FocusEvent) => {
      if (!dialog.current?.contains(event.target as Node)) dialog.current?.focus();
    };
    const keydown = (event: KeyboardEvent) => {
      if (event.key === "Escape") { event.preventDefault(); event.stopPropagation(); if (!busy.current) close.current(); }
      if (event.key === "Tab") {
        const elements = Array.from(dialog.current?.querySelectorAll<HTMLElement>('button:not(:disabled), input:not(:disabled)') ?? []);
        const index = elements.indexOf(document.activeElement as HTMLElement);
        const next = event.shiftKey ? (index <= 0 ? elements.length - 1 : index - 1) : (index + 1) % elements.length;
        event.preventDefault(); event.stopPropagation(); (elements[next] ?? dialog.current)?.focus();
      }
    };
    document.addEventListener("keydown", keydown, true);
    document.addEventListener("focusin", focus);
    return () => {
      document.removeEventListener("keydown", keydown, true); document.removeEventListener("focusin", focus);
      if (shell) shell.inert = inert ?? false;
      if (main) main.style.overflow = overflow;
      document.body.style.overflow = bodyOverflow;
      // Context menu items disappear on opening; fall back to the library search.
      const restore = previous?.isConnected && previous !== document.body && previous !== document.documentElement
        ? previous : document.querySelector<HTMLElement>('[aria-label="搜索音频库"]');
      restore?.focus({ preventScroll: true });
    };
  }, []);
  async function confirm() {
    if (busy.current) return;
    busy.current = true; setPending(true); setError("");
    try {
      await onConfirm(deleteFiles);
      if (skipNext && !deleteFiles) {
        try { localStorage.setItem(SKIP_LIBRARY_REMOVAL_KEY, "true"); } catch { /* Storage unavailable: continue asking next time. */ }
      }
      close.current();
    } catch (e) { setError(normalizeAppError(e).message); }
    finally { busy.current = false; setPending(false); }
  }
  return createPortal(<div className="library-dialog-backdrop" onClick={event => { if (event.target === event.currentTarget && !pending) onClose(); }}>
    <div className="modal-card library-remove-dialog" ref={dialog} role="dialog" aria-modal="true" aria-labelledby={titleId} aria-describedby={descriptionId}
      aria-busy={pending} tabIndex={-1} onKeyDown={event => event.stopPropagation()}>
      <header className="library-remove-header"><h2 id={titleId}>从音频库移除作品</h2>
        <button type="button" className="btn-icon" aria-label="关闭移除确认" title="关闭移除确认 (Esc)" disabled={pending} onClick={onClose}><X size={18} /></button>
      </header>
      <p className="library-remove-title">{work.title}</p>
      <p id={descriptionId} className="library-remove-description">将移除作品索引、播放队列项与关联记录。<strong>本地导入的原始音频文件会保留。</strong></p>
      <div className="library-remove-options">
        {work.hasDownloaded && <label className="library-dialog-check">
          <input type="checkbox" checked={deleteFiles} disabled={pending} onChange={event => { setDeleteFiles(event.target.checked); if (event.target.checked) setSkipNext(false); }} />
          <span>同时删除已下载文件<small>仅删除软件下载目录内的副本及未完成下载文件。</small></span>
        </label>}
        <label className="library-dialog-check">
          <input type="checkbox" checked={skipNext} disabled={pending || deleteFiles} onChange={event => setSkipNext(event.target.checked)} />
          <span>下次不再提醒<small>之后直接移除作品索引，保留音频文件。</small></span>
        </label>
      </div>
      {error && <p className="error-banner" role="alert">{error}</p>}
      <footer className="library-remove-footer">
        <button ref={cancel} type="button" className="btn btn-ghost" disabled={pending} onClick={onClose}>取消</button>
        <button type="button" className="btn btn-danger" disabled={pending} onClick={() => void confirm()}><Trash2 size={14} /><span className="ui-icon-label">
          {pending ? "正在移除…" : deleteFiles ? "移除并删除下载文件" : "移除作品索引"}
        </span></button>
      </footer>
    </div>
  </div>, document.body);
}
