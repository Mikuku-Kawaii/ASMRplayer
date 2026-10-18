import { ReactNode, useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { AlertTriangle, Check, X } from "lucide-react";

export function Toast({ message, onClose, duration = 3000, tone = "success", children }: {
  message: string; onClose: () => void; duration?: number; tone?: "success" | "error" | "warning"; children?: ReactNode;
}) {
  const [closing, setClosing] = useState(false);
  const anchor = useRef<HTMLSpanElement>(null);
  const popup = useRef<HTMLDivElement>(null);
  const [host, setHost] = useState<HTMLElement>(document.body);
  const close = useRef(onClose);
  close.current = onClose;
  useLayoutEffect(() => {
    const dialogs = document.querySelectorAll<HTMLElement>('dialog[open], [role="dialog"][aria-modal="true"]');
    setHost(anchor.current?.closest<HTMLElement>('dialog, [role="dialog"], [role="menu"]') ?? dialogs[dialogs.length - 1] ?? document.body);
  }, []);
  useEffect(() => {
    const node = popup.current;
    node?.setAttribute("popover", "manual");
    node?.showPopover?.();
    return () => { if (node?.isConnected) node.hidePopover?.(); };
  }, [host]);
  useEffect(() => {
    if (duration <= 0) return;
    const timer = setTimeout(() => setClosing(true), duration);
    return () => clearTimeout(timer);
  }, [duration]);
  useEffect(() => {
    if (!closing) return;
    const timer = setTimeout(() => close.current(), window.matchMedia?.("(prefers-reduced-motion: reduce)").matches ? 0 : 180);
    return () => clearTimeout(timer);
  }, [closing]);
  const Icon = tone === "success" ? Check : AlertTriangle;
  return <><span ref={anchor} hidden />{createPortal(<div ref={popup} className={`app-toast is-${tone}${closing ? " is-closing" : ""}`}
    onKeyDown={event => { event.stopPropagation(); if (event.key === "Escape") { event.preventDefault(); setClosing(true); } }}>
    <Icon size={18} aria-hidden="true" />
    <div className="toast-content"><span className="ui-icon-label" role={tone === "success" ? "status" : "alert"} aria-live={tone === "success" ? "polite" : "assertive"} aria-atomic="true">{message}</span>
      {children && <div className="toast-actions">{children}</div>}
    </div>
    <button type="button" className="btn btn-icon toast-close" aria-label="关闭提示" title="关闭提示" disabled={closing} onClick={() => setClosing(true)}><X size={16} aria-hidden="true" /></button>
  </div>, host)}</>;
}
