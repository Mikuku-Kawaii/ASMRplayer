import { useLayoutEffect, useRef } from "react";

// T1414: depend on content/available width, never playback ticks. WAAPI keeps motion
// off React's render loop; endpoints pause for reading before looping or reversing.
export function ScrollingText({ text, identity, className, loop = false }: { text: string; identity: string; className: string; loop?: boolean }) {
  const viewport = useRef<HTMLSpanElement>(null);
  const content = useRef<HTMLSpanElement>(null);
  useLayoutEffect(() => {
    const box = viewport.current;
    const line = content.current;
    if (!box || !line) return;
    const media = window.matchMedia?.("(prefers-reduced-motion: reduce)");
    let animation: Animation | undefined;
    let distance = -1;
    let reduced = false;
    const measure = () => {
      const next = Math.max(0, line.scrollWidth - box.clientWidth);
      if (distance === next && reduced === !!media?.matches) return;
      distance = next; reduced = !!media?.matches; animation?.cancel();
      if (next < 2 || reduced || !line.animate) return;
      const travel = next / 28 * 1000;
      const duration = (loop ? travel : 2 * travel) + 2800;
      const pause = 1400 / duration;
      animation = line.animate(loop ? [
        { transform: "translateX(0)", offset: 0 },
        { transform: "translateX(0)", offset: pause },
        { transform: `translateX(-${next}px)`, offset: 1 - pause },
        { transform: `translateX(-${next}px)`, offset: 1 },
      ] : [
        { transform: "translateX(0)", offset: 0 },
        { transform: "translateX(0)", offset: pause },
        { transform: `translateX(-${next}px)`, offset: .5 },
        { transform: `translateX(-${next}px)`, offset: .5 + pause },
        { transform: "translateX(0)", offset: 1 },
      ], { duration, iterations: Infinity, easing: "linear", direction: "normal" });
    };
    const observer = typeof ResizeObserver !== "undefined" ? new ResizeObserver(measure) : undefined;
    observer?.observe(box); observer?.observe(line); measure();
    media?.addEventListener?.("change", measure);
    return () => { observer?.disconnect(); media?.removeEventListener?.("change", measure); animation?.cancel(); };
  }, [text, identity, loop]);
  return <span ref={viewport} className={`${className} island-scrolling-text`} aria-label={text}>
    <span key={`${identity}:${text}`} className="island-text-transition" aria-hidden="true">
      <span ref={content} className="island-scrolling-line">{text}</span>
    </span>
  </span>;
}
