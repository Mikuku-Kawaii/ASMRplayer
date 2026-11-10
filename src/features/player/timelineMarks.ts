import { useEffect, useState } from "react";
import { callCommand, type BookmarkDto } from "../../lib/api";

const changed = new EventTarget();
export function notifyTimelineMarksChanged() {
  changed.dispatchEvent(new Event("change"));
}

export function useTimelineMarks(versionId: string | null | undefined) {
  const [result, setResult] = useState<{ versionId: string; positions: number[] }>();
  useEffect(() => {
    if (!versionId) return;
    let generation = 0;
    const load = async () => {
      const ticket = ++generation;
      try {
        const rows = await callCommand<BookmarkDto[]>("list_bookmarks", { versionId });
        if (ticket === generation) setResult({ versionId, positions: [...new Set(rows.map(b => b.positionMs))] });
      } catch {
        if (ticket === generation) setResult(undefined);
      }
    };
    void load();
    changed.addEventListener("change", load);
    return () => { ++generation; changed.removeEventListener("change", load); };
  }, [versionId]);
  return result && result.versionId === versionId ? result.positions : [];
}
