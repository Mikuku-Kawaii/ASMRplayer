import { RotateCcw, RotateCw } from "lucide-react";

/** Decorative glyph; the enclosing button supplies the action's accessible name. */
export function SeekFiveIcon({ direction }: { direction: "back" | "forward" }) {
  const Arrow = direction === "back" ? RotateCcw : RotateCw;
  return <span className="seek-five-icon" aria-hidden="true"><Arrow /><span className="seek-five-number">5</span></span>;
}
