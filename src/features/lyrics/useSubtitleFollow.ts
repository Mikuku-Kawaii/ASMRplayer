import { useCallback, useLayoutEffect, useRef, useState } from "react";

// Only real input suspends follow; programmatic scroll events never extend the timer.
export function useSubtitleFollow(activeIndex: number, versionId: string | null | undefined, open: boolean) {
  const container = useRef<HTMLDivElement>(null);
  const timer = useRef<ReturnType<typeof setTimeout>>();
  const [paused, setPaused] = useState(false);
  const [resume, setResume] = useState(0);
  const dragging = useRef(false);
  const initialPosition = useRef(true);
  const frame = useRef<number>();
  const stopAnimation = useCallback(() => {
    if (frame.current !== undefined) cancelAnimationFrame(frame.current);
    frame.current = undefined;
  }, []);
  const suspend = useCallback(() => {
    stopAnimation();
    setPaused(true); clearTimeout(timer.current);
    timer.current = setTimeout(() => { setPaused(false); setResume(x => x + 1); }, 3000);
  }, [stopAnimation]);
  useLayoutEffect(() => {
    initialPosition.current = true;
    dragging.current = false;
    clearTimeout(timer.current); setPaused(false);
    return () => clearTimeout(timer.current);
  }, [versionId, open]);
  useLayoutEffect(() => {
    if (!open || paused || activeIndex < 0) return;
    const view = container.current;
    const line = view?.querySelector<HTMLElement>(`[data-cue-index="${activeIndex}"]`);
    if (!view || !line) return;
    // Space at both ends allows even the first and last cue to reach the center.
    view.style.setProperty("--subtitle-follow-padding", `${view.clientHeight / 2}px`);
    // Re-measure while the active cue's font size changes, so the final position stays centered.
    const target = () => line.offsetTop - (view.clientHeight - line.offsetHeight) / 2;
    const instant = initialPosition.current || window.matchMedia?.("(prefers-reduced-motion: reduce)").matches;
    initialPosition.current = false;
    if (instant) {
      view.scrollTo?.({ top: target(), behavior: "instant" });
      return;
    }
    const from = view.scrollTop;
    const started = performance.now();
    const animate = (now: number) => {
      const progress = Math.min(1, Math.max(0, (now - started) / 640));
      const destination = target();
      const distance = destination - from;
      // A damped spring with a soft start and at most 6px of overshoot, even after a long drag.
      const spring = 1 - Math.exp(-9 * progress) * (Math.cos(8 * progress) + 9 / 8 * Math.sin(8 * progress));
      const extra = spring > 1 ? Math.sign(distance) * Math.min(6, Math.abs(distance) * (spring - 1)) : 0;
      const top = progress === 1 ? destination : from + distance * Math.min(1, spring) + extra;
      view.scrollTo?.({ top, behavior: "instant" });
      frame.current = progress < 1 ? requestAnimationFrame(animate) : undefined;
    };
    animate(started);
    return stopAnimation;
  }, [activeIndex, versionId, open, paused, resume, stopAnimation]);
  return { ref: container, onWheel: suspend, onTouchMove: suspend,
    onPointerDown: () => { dragging.current = true; suspend(); },
    onPointerMove: (e: React.PointerEvent) => { if (dragging.current && e.buttons) suspend(); else dragging.current = false; },
    onPointerUp: () => { dragging.current = false; suspend(); },
    onPointerCancel: () => { dragging.current = false; suspend(); },
    onKeyDown: (e: React.KeyboardEvent) => { if (["ArrowUp", "ArrowDown", "PageUp", "PageDown", "Home", "End", " "].includes(e.key)) { e.stopPropagation(); suspend(); } },
  };
}
