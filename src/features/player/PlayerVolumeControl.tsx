import { useEffect, useId, useRef, useState, type CSSProperties } from "react";
import { Volume2, VolumeX } from "lucide-react";

/** Shares the player's volume command; never owns a second playback state. */
export function PlayerVolumeControl({ volume, onChange }: {
  volume: number; onChange: (volume: number) => Promise<void>;
}) {
  const [open, setOpen] = useState(false);
  const previous = useRef(volume > 0 ? volume : 85);
  const root = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const slider = useRef<HTMLInputElement>(null);
  const focusSlider = useRef(false);
  const returningFocus = useRef(false);
  const id = useId();
  useEffect(() => { if (volume > 0) previous.current = volume; }, [volume]);
  useEffect(() => {
    if (!open) return;
    if (focusSlider.current) { slider.current?.focus(); focusSlider.current = false; }
    const outside = (event: PointerEvent) => {
      if (!root.current?.contains(event.target as Node)) setOpen(false);
    };
    const escape = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.stopPropagation();
      setOpen(false);
      // Hover must not steal focus; only return focus if it was inside this control.
      if (root.current?.contains(document.activeElement)) {
        returningFocus.current = true;
        trigger.current?.focus();
        returningFocus.current = false;
      }
    };
    document.addEventListener("pointerdown", outside);
    document.addEventListener("keydown", escape, true);
    return () => { document.removeEventListener("pointerdown", outside); document.removeEventListener("keydown", escape, true); };
  }, [open]);
  return <div className="player-volume" ref={root} role="group" aria-label="音量控制"
    onMouseEnter={() => setOpen(true)}
    onMouseLeave={() => { if (!root.current?.contains(document.activeElement)) setOpen(false); }}
    onBlur={event => { if (!event.currentTarget.contains(event.relatedTarget as Node | null)) setOpen(false); }}>
    <button ref={trigger} type="button" className="btn-icon" aria-label={volume === 0 ? "取消静音" : "静音"}
      title={`音量 ${Math.round(volume)}% · 悬停调节，点击${volume === 0 ? "取消静音" : "静音"}`}
      aria-pressed={volume === 0} aria-expanded={open} aria-controls={open ? id : undefined}
      onFocus={() => { if (!returningFocus.current) setOpen(true); }}
      onKeyDown={event => {
        if (event.key === "ArrowUp" || event.key === "ArrowDown") {
          event.preventDefault(); event.stopPropagation();
          if (open) slider.current?.focus();
          else { focusSlider.current = true; setOpen(true); }
        }
      }}
      onClick={() => void onChange(volume > 0 ? 0 : previous.current)}>
      {volume === 0 ? <VolumeX size={21} /> : <Volume2 size={21} />}
    </button>
    {open && <div id={id} className="player-volume-popup" role="group" aria-label="调整音量">
      <input ref={slider} type="range" className="volume-slider" aria-label="音量" aria-orientation="vertical"
        aria-valuetext={`${Math.round(volume)}%`} min={0} max={200} step={1} value={Math.round(volume)}
        title="音量 0–200%"
        style={{ "--volume-fill": `${Math.max(0, Math.min(100, volume / 2))}%` } as CSSProperties}
        onChange={event => void onChange(Number(event.target.value))} />
      <span className="player-volume-value">{Math.round(volume)}%</span>
    </div>}
  </div>;
}
