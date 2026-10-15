import { useEffect, useState } from "react";
import { AppSettingsDto, callCommand, subscribeSettingsUpdated } from "../../lib/api";

/** Synchronize hidden and visible entry points with the persisted host setting. */
export function useNsfwSetting() {
  const [enabled, setEnabled] = useState<boolean | null>(null);
  useEffect(() => {
    let disposed = false;
    let received = false;
    let stop: (() => void) | undefined;
    const read = () => callCommand<AppSettingsDto>("get_app_settings").then(settings => {
      if (!disposed && !received && settings) setEnabled(settings.nsfwEnabled ?? false);
    });
    void subscribeSettingsUpdated(settings => {
      received = true;
      if (!disposed) setEnabled(settings.nsfwEnabled ?? false);
    }).then(unlisten => {
      if (disposed) { unlisten(); return; }
      stop = unlisten;
      return read();
    }, () => { if (!disposed) return read(); }).catch(() => {});
    return () => { disposed = true; stop?.(); };
  }, []);
  return [enabled, setEnabled] as const;
}
