import { useLayoutEffect, useRef, useState } from "react";

const resting: Keyframe = { opacity: 1, transform: "none" };
const hidden: Keyframe = { opacity: 0, transform: "translateX(8px)" };

export function useImmersionDrawerMotion(open: boolean, canRetain: boolean) {
  const [retain, setRetain] = useState(false);
  const panelRef = useRef<HTMLElement>(null);
  const closeRef = useRef<HTMLButtonElement>(null);
  const motion = useRef<Animation>();

  useLayoutEffect(() => {
    const preference = window.matchMedia?.("(prefers-reduced-motion: reduce)");
    const reduceMotion = () => {
      if (!preference?.matches) return;
      motion.current?.cancel();
      motion.current = undefined;
      setRetain(false);
    };
    preference?.addEventListener?.("change", reduceMotion);
    return () => {
      motion.current?.cancel();
      preference?.removeEventListener?.("change", reduceMotion);
    };
  }, []);
  useLayoutEffect(() => {
    const panel = panelRef.current;
    const previous = motion.current;
    const style = previous && panel ? getComputedStyle(panel) : undefined;
    const from = style ? { opacity: style.opacity, transform: style.transform } : open ? hidden : resting;
    previous?.cancel();
    motion.current = undefined;
    if (panel) panel.inert = !open && canRetain;
    if (open) closeRef.current?.focus({ preventScroll: true });

    if ((!open && !canRetain) || !panel?.animate ||
        window.matchMedia?.("(prefers-reduced-motion: reduce)").matches) {
      setRetain(false);
      return;
    }

    setRetain(true);
    const animation = panel.animate([from, open ? resting : hidden], {
      duration: open ? 160 : 120,
      easing: open ? "ease-out" : "ease-in",
      fill: "both",
    });
    motion.current = animation;
    void animation.finished.then(() => {
      animation.cancel();
      motion.current = undefined;
      if (!open) setRetain(false);
    }, () => { /* A new transition took over. */ });
  }, [open, canRetain]);

  return { retain, panelRef, closeRef };
}
