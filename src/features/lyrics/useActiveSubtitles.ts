import { useEffect, useState } from "react";
import { callCommand, subscribeActiveSubtitles, type SubtitleCueDto } from "../../lib/api";

/** Every window consumes the Rust-owned selection, including generated bilingual cues. */
export function useActiveSubtitles(versionId: string | null | undefined, refresh = 0) {
  const [cues, setCues] = useState<SubtitleCueDto[]>([]);
  useEffect(() => {
    let disposed = false;
    let sequence = -1;
    let stop: (() => void) | undefined;
    setCues([]);
    if (!versionId) return;
    const accept = (next: { eventSeq: number; versionId: string; cues: SubtitleCueDto[] }) => {
      if (!disposed && next?.versionId === versionId && next.eventSeq >= sequence) {
        sequence = next.eventSeq;
        setCues(next.cues);
      }
    };
    void subscribeActiveSubtitles(accept).then(async fn => {
      if (disposed) { fn(); return; }
      stop = fn;
      accept(await callCommand("get_active_subtitles"));
    }).catch(() => {});
    return () => { disposed = true; stop?.(); };
  }, [versionId, refresh]);
  return cues;
}
