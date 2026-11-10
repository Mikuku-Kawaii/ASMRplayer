import { useId, useRef, useState, type CSSProperties, type PointerEvent } from "react";
import { formatDurationMs } from "../../lib/api";

/** Pointer preview is local to this control; it never changes the playback position. */
export function IslandSeekBar({ positionMs, durationMs, onPreview, onCommit }: {
  positionMs: number; durationMs?: number | null; onPreview: (value: number | null) => void; onCommit: () => void;
}) {
  const input = useRef<HTMLInputElement>(null);
  const tooltipId = useId();
  const [pointerRatio, setPointerRatio] = useState(0);
  const [inside, setInside] = useState(false);
  const [dragging, setDragging] = useState(false);
  const [keyboard, setKeyboard] = useState(false);
  const duration = durationMs || 0;
  const progress = Math.max(0, Math.min(1, positionMs / (duration || 1)));
  const active = duration > 0 && (inside || dragging || keyboard);
  const previewRatio = dragging || keyboard ? progress : pointerRatio;

  function point(event: PointerEvent) {
    const element = input.current;
    if (!duration || !element || event.pointerType === "touch") return;
    setKeyboard(false);
    const rect = element.getBoundingClientRect();
    // Native range values follow the thumb center, including the capsule's scale.
    const inset = Math.min(rect.width / 2, 8 * rect.width / (element.offsetWidth || rect.width || 1));
    setPointerRatio(Math.max(0, Math.min(1, (event.clientX - rect.left - inset) / (rect.width - 2 * inset || 1))));
  }

  return <div className={`island-seek ${active ? "is-active" : ""}`} style={{
    "--island-progress": `${progress * 100}%`, "--island-hover-x": `${previewRatio * 100}%`,
  } as CSSProperties}
    onPointerEnter={event => { if (event.pointerType !== "touch") { setInside(true); point(event); } }}
    onPointerMove={point} onPointerLeave={() => setInside(false)}>
    <div className="island-progress-track" aria-hidden="true"><span /></div>
    <div id={tooltipId} role="tooltip" aria-hidden={!active} className="island-seek-preview tabular-nums">
      {formatDurationMs(Math.round(previewRatio * duration))}
    </div>
    <input ref={input} type="range" aria-label="播放进度" aria-valuetext={`${formatDurationMs(positionMs)} / ${formatDurationMs(durationMs)}`}
      aria-describedby={active ? tooltipId : undefined} min={0} max={duration || 1} step={1000}
      value={Math.min(positionMs, duration || 1)} disabled={!duration}
      onChange={event => onPreview(Number(event.target.value))}
      onPointerDown={event => {
        if (!duration || event.button !== 0) return;
        setKeyboard(false); setDragging(true); point(event);
        event.currentTarget.setPointerCapture?.(event.pointerId);
      }}
      onPointerUp={() => { setDragging(false); onCommit(); }}
      onPointerCancel={() => { setDragging(false); setInside(false); onPreview(null); }}
      onFocus={event => setKeyboard(event.currentTarget.matches(":focus-visible"))}
      onKeyDown={event => { if (["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown", "Home", "End", "PageUp", "PageDown"].includes(event.key)) setKeyboard(true); }}
      onKeyUp={onCommit} onBlur={() => { setKeyboard(false); onCommit(); }} />
  </div>;
}
