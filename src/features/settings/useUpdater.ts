import { useCallback, useEffect, useRef, useState } from "react";
import { callCommand, subscribeUpdateSnapshot, UpdateSnapshot } from "../../lib/api";

// Subscribe before fetching so neither a fast startup check nor a late IPC response
// can replace a newer download/installation status. Safe with React StrictMode.
export function useUpdater() {
  const [snapshot, setSnapshot] = useState<UpdateSnapshot | null>(null);
  const sequence = useRef(-1);
  const apply = useCallback((next: UpdateSnapshot | null | undefined) => {
    if (next && next.eventSeq >= sequence.current) {
      sequence.current = next.eventSeq;
      setSnapshot(next);
    }
  }, []);
  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void (async () => {
      const stop = await subscribeUpdateSnapshot((next) => { if (!disposed) apply(next); });
      if (disposed) { stop(); return; }
      unlisten = stop;
      const next = await callCommand<UpdateSnapshot>("get_update_snapshot");
      if (!disposed) apply(next);
    })().catch(() => {});
    return () => { disposed = true; unlisten?.(); };
  }, [apply]);
  const run = useCallback(async (command: string, args?: Record<string, unknown>) => {
    const next = await callCommand<UpdateSnapshot>(command, args);
    apply(next);
    return next;
  }, [apply]);
  return { snapshot, run };
}
