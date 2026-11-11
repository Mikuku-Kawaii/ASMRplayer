import { RefObject, useLayoutEffect } from "react";

export type DialogSize = "compact" | "standard" | "wide";

/** Dialogs share the content area's centre and leave navigation/player space visible. */
export function useDialogGeometry(ref: RefObject<HTMLElement>, size: DialogSize) {
  useLayoutEffect(() => {
    const node = ref.current;
    const content = document.querySelector<HTMLElement>(".main-content");
    const position = () => {
      if (!node) return;
      let zoom = 1;
      for (let parent: HTMLElement | null = node; parent; parent = parent.parentElement) {
        zoom *= Number.parseFloat(getComputedStyle(parent).getPropertyValue("zoom")) || 1;
      }
      const rect = content?.getBoundingClientRect();
      const left = Math.max(0, rect?.left ?? 0);
      const top = Math.max(0, rect?.top ?? 0);
      const right = Math.min(window.innerWidth, rect?.right ?? window.innerWidth);
      const bottom = Math.min(window.innerHeight, rect?.bottom ?? window.innerHeight);
      Object.assign(node.style, {
        position: "fixed", margin: "0", inset: "auto", translate: "-50% -50%", transform: "none",
        left: `${(left + right) / 2 / zoom}px`, top: `${(top + bottom) / 2 / zoom}px`,
        width: `${Math.min({ compact: 400, standard: 560, wide: 800 }[size], Math.max(0, (right - left) / zoom - 32))}px`,
        maxHeight: `${Math.max(0, Math.min((bottom - top) / zoom - 32, (bottom - top) / zoom * .8))}px`,
      });
    };
    position();
    const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(position);
    if (content) observer?.observe(content);
    const scaleObserver = new MutationObserver(position);
    scaleObserver.observe(document.documentElement, { attributes: true, attributeFilter: ["style", "class"] });
    scaleObserver.observe(document.body, { attributes: true, attributeFilter: ["style", "class"] });
    window.addEventListener("resize", position);
    return () => { observer?.disconnect(); scaleObserver.disconnect(); window.removeEventListener("resize", position); };
  }, [ref, size]);
}
