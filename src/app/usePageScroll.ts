import { useLayoutEffect, type RefObject } from "react";

// Each scroll owner has its own key. Restoration waits for asynchronously loaded
// content to grow, and never overwrites the saved target with a loading-time clamp.
export function usePageScroll(ref: RefObject<HTMLElement>, page: string | null) {
  useLayoutEffect(() => {
    const element = ref.current;
    if (!element || !page) return;
    const key = `asmr-player.page-scroll.v1.${page}`;
    let target = 0;
    try {
      const saved = Number(localStorage.getItem(key));
      if (Number.isFinite(saved) && saved >= 0) target = saved;
    } catch { /* Storage may be unavailable. */ }
    let restoring = true;
    let frame = 0;
    let position = target;
    const persist = () => {
      try { localStorage.setItem(key, String(position)); } catch { /* Keep scrolling usable. */ }
    };
    const save = () => {
      if (restoring) return;
      if (element.clientHeight === 0) return;
      position = element.scrollTop;
      persist();
    };
    const restore = () => {
      if (!restoring) return;
      element.scrollTop = target;
      if (Math.abs(element.scrollTop - target) < 1) restoring = false;
    };
    const scheduleRestore = () => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(restore);
    };
    const userInput = () => { restoring = false; save(); };
    const keyInput = (event: KeyboardEvent) => {
      if (["ArrowUp", "ArrowDown", "PageUp", "PageDown", "Home", "End", " "].includes(event.key)
        && !(event.target instanceof Element && event.target.closest("input, textarea, [contenteditable=true]"))) userInput();
    };
    const resize = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(scheduleRestore);
    const observeChildren = () => {
      resize?.disconnect(); resize?.observe(element);
      Array.from(element.children).forEach(child => resize?.observe(child));
      scheduleRestore();
    };
    const mutations = new MutationObserver(observeChildren);
    mutations.observe(element, { childList: true, subtree: true });
    element.addEventListener("scroll", save, { passive: true });
    element.addEventListener("wheel", userInput, { passive: true });
    element.addEventListener("touchstart", userInput, { passive: true });
    element.addEventListener("pointerdown", userInput);
    element.addEventListener("keydown", keyInput);
    window.addEventListener("pagehide", persist);
    if (target === 0) restore();
    observeChildren();
    return () => {
      // DOM can already have shrunk when layout cleanup runs. Keep the last
      // observed position instead of saving that transient clamp to zero.
      persist(); cancelAnimationFrame(frame); resize?.disconnect(); mutations.disconnect();
      element.removeEventListener("scroll", save); element.removeEventListener("wheel", userInput);
      element.removeEventListener("touchstart", userInput); element.removeEventListener("pointerdown", userInput);
      element.removeEventListener("keydown", keyInput); window.removeEventListener("pagehide", persist);
    };
  }, [page, ref]);
}
