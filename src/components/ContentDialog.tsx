import { ReactNode, useLayoutEffect, useRef } from "react";
import { createPortal } from "react-dom";

export function ContentDialog({ children, onClose, busy = false, label }: { children: ReactNode; onClose: () => void; busy?: boolean; label: string }) {
  const dialog = useRef<HTMLDialogElement>(null);
  useLayoutEffect(() => {
    const previous = document.activeElement as HTMLElement | null;
    const node = dialog.current;
    const overflow = document.body.style.overflow;
    document.body.style.overflow = "hidden";
    const content = document.querySelector<HTMLElement>(".main-content");
    const position = () => {
      if (!node) return;
      const rect = content?.getBoundingClientRect();
      const left = Math.max(0, rect?.left ?? 0);
      const top = Math.max(0, rect?.top ?? 0);
      const right = Math.min(window.innerWidth, rect?.right ?? window.innerWidth);
      const bottom = Math.min(window.innerHeight, rect?.bottom ?? window.innerHeight);
      node.style.left = `${(left + right) / 2}px`;
      node.style.top = `${(top + bottom) / 2}px`;
      node.style.width = `${Math.min(760, Math.max(0, right - left - 32))}px`;
      node.style.maxHeight = `${Math.max(0, bottom - top - 32)}px`;
    };
    position();
    const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(position);
    if (content) observer?.observe(content);
    window.addEventListener("resize", position);
    node?.showModal();
    return () => {
      observer?.disconnect();
      window.removeEventListener("resize", position);
      node?.close();
      document.body.style.overflow = overflow;
      if (previous?.isConnected) previous.focus({ preventScroll: true });
    };
  }, []);
  return createPortal(<dialog ref={dialog} className="modal-card import-dialog" aria-label={label} aria-busy={busy}
    onCancel={event => { event.preventDefault(); if (!busy) onClose(); }} onKeyDown={event => event.stopPropagation()}>
    {children}
  </dialog>, document.body);
}
