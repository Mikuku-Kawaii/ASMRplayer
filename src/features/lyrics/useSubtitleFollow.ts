import { useCallback, useEffect, useRef, useState } from "react";

// Only real input suspends follow; programmatic scroll events never extend the timer.
export function useSubtitleFollow(activeIndex: number, versionId: string | null | undefined, open: boolean) {
  const container = useRef<HTMLDivElement>(null);
  const timer = useRef<ReturnType<typeof setTimeout>>();
  const [paused, setPaused] = useState(false);
  const [resume, setResume] = useState(0);
  const dragging = useRef(false);
  const suspend = useCallback(() => {
    setPaused(true); clearTimeout(timer.current);
    timer.current = setTimeout(() => { setPaused(false); setResume(x => x + 1); }, 5000);
  }, []);
  useEffect(() => {
    clearTimeout(timer.current); setPaused(false); setResume(x => x + 1);
    return () => clearTimeout(timer.current);
  }, [versionId, open]);
  useEffect(() => {
    if (!open || paused || activeIndex < 0) return;
    const view = container.current;
    const line = view?.querySelector<HTMLElement>(`[data-cue-index="${activeIndex}"]`);
    if (!view || !line) return;
    const box = view.getBoundingClientRect(); const row = line.getBoundingClientRect();
    view.scrollTo?.({ top: view.scrollTop + row.top - box.top - (view.clientHeight - row.height) / 2,
      behavior: window.matchMedia?.("(prefers-reduced-motion: reduce)").matches ? "auto" : "smooth" });
  }, [activeIndex, open, paused, resume]);
  return { ref: container, onWheel: suspend, onTouchMove: suspend,
    onPointerDown: () => { dragging.current = true; suspend(); },
    onPointerMove: (e: React.PointerEvent) => { if (dragging.current && e.buttons) suspend(); else dragging.current = false; },
    onPointerUp: () => { dragging.current = false; suspend(); },
    onPointerCancel: () => { dragging.current = false; suspend(); },
    onKeyDown: (e: React.KeyboardEvent) => { if (["ArrowUp", "ArrowDown", "PageUp", "PageDown", "Home", "End", " "].includes(e.key)) { e.stopPropagation(); suspend(); } },
  };
}
