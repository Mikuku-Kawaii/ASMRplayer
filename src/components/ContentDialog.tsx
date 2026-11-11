import { ReactNode, useLayoutEffect, useRef } from "react";
import { createPortal } from "react-dom";
import { DialogSize, useDialogGeometry } from "./useDialogGeometry";

export function ContentDialog({ children, onClose, busy = false, label, size = "standard", className = "" }: { children: ReactNode; onClose: () => void; busy?: boolean; label: string; size?: DialogSize; className?: string }) {
  const dialog = useRef<HTMLDialogElement>(null);
  useDialogGeometry(dialog, size);
  useLayoutEffect(() => {
    const previous = document.activeElement as HTMLElement | null;
    const node = dialog.current;
    const overflow = document.body.style.overflow;
    const main = document.querySelector<HTMLElement>(".main-content");
    const mainOverflow = main?.style.overflow ?? "";
    const wasLocked = main?.classList.contains("is-scroll-locked");
    const bodyWasLocked = document.body.classList.contains("is-modal-scroll-locked");
    if (main) { main.style.overflow = "hidden"; main.classList.add("is-scroll-locked"); }
    document.body.classList.add("is-modal-scroll-locked");
    document.body.style.overflow = "hidden";
    node?.showModal();
    return () => {
      node?.close();
      document.body.style.overflow = overflow;
      if (main) { main.style.overflow = mainOverflow; if (!wasLocked) main.classList.remove("is-scroll-locked"); }
      if (!bodyWasLocked) document.body.classList.remove("is-modal-scroll-locked");
      if (previous?.isConnected) previous.focus({ preventScroll: true });
    };
  }, []);
  return createPortal(<dialog ref={dialog} className={`modal-card import-dialog content-dialog ${className}`} data-dialog-size={size} aria-label={label} aria-busy={busy}
    onCancel={event => { event.preventDefault(); if (!busy) onClose(); }} onKeyDown={event => event.stopPropagation()}
    onClick={event => {
      if (busy || event.target !== event.currentTarget) return;
      const rect = event.currentTarget.getBoundingClientRect();
      if (event.clientX < rect.left || event.clientX > rect.right || event.clientY < rect.top || event.clientY > rect.bottom) onClose();
    }}>
    {children}
  </dialog>, document.body);
}
