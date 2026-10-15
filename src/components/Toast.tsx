import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { Check, X } from "lucide-react";

export function Toast({ message, onClose, duration = 3000 }: { message: string; onClose: () => void; duration?: number }) {
  const [closing, setClosing] = useState(false);
  const close = useRef(onClose);
  close.current = onClose;
  useEffect(() => {
    const timer = setTimeout(() => setClosing(true), duration);
    return () => clearTimeout(timer);
  }, [duration]);
  useEffect(() => {
    if (!closing) return;
    const timer = setTimeout(() => close.current(), window.matchMedia?.("(prefers-reduced-motion: reduce)").matches ? 0 : 180);
    return () => clearTimeout(timer);
  }, [closing]);
  return createPortal(<div className={`app-toast${closing ? " is-closing" : ""}`}>
    <Check size={18} aria-hidden="true" />
    <span className="ui-icon-label" role="status" aria-live="polite" aria-atomic="true">{message}</span>
    <button type="button" className="btn btn-icon toast-close" aria-label="关闭提示" title="关闭提示" disabled={closing} onClick={() => setClosing(true)}><X size={16} aria-hidden="true" /></button>
  </div>, document.body);
}
