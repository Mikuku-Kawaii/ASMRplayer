import { useCallback, useEffect, useRef } from "react";
import type { FocusEvent, MutableRefObject, PointerEvent } from "react";
import type { IslandForm } from "../../lib/api";

/** Mouse exit wins over focus left behind by clicking a playback control. */
export function useIslandExpansion(mode: "focus" | "click", form: IslandForm,
  changeForm: (form: IslandForm) => void, dragging: MutableRefObject<boolean>) {
  const inside = useRef(false);
  const pressed = useRef(false);
  const requested = useRef<IslandForm | null>(null);
  useEffect(() => { requested.current = null; }, [form, mode]);
  const request = useCallback((next: "compact" | "expanded") => {
    if (mode !== "focus" || dragging.current || requested.current === next) return;
    if (form === next && requested.current === null) return;
    if (next === "expanded" && form === "menu") return;
    requested.current = next;
    changeForm(next);
  }, [mode, form, changeForm, dragging]);
  const release = useCallback(() => {
    pressed.current = false;
    if (!inside.current) request("compact");
  }, [request]);
  useEffect(() => {
    const blur = () => { inside.current = false; release(); };
    window.addEventListener("pointerup", release);
    window.addEventListener("pointercancel", release);
    window.addEventListener("blur", blur);
    return () => {
      window.removeEventListener("pointerup", release);
      window.removeEventListener("pointercancel", release);
      window.removeEventListener("blur", blur);
    };
  }, [release]);
  return {
    onPointerEnter: (event: PointerEvent) => {
      inside.current = true;
      if (event.pointerType !== "touch" && !event.buttons) request("expanded");
    },
    onPointerLeave: () => { inside.current = false; if (!pressed.current) request("compact"); },
    onPointerDownCapture: () => { pressed.current = true; },
    onFocus: (event: FocusEvent) => {
      if (!(event.relatedTarget instanceof Node) || !event.currentTarget.contains(event.relatedTarget)) request("expanded");
    },
    onBlur: (event: FocusEvent) => {
      if (!inside.current && !pressed.current && (!(event.relatedTarget instanceof Node) || !event.currentTarget.contains(event.relatedTarget))) request("compact");
    },
    onDragEnd: release,
  };
}
