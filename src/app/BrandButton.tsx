import { useRef, useState } from "react";
import { AppError, callCommand, normalizeAppError } from "../lib/api";
import { useNsfwSetting } from "../features/settings/useNsfwSetting";

export function BrandButton({ onError }: { onError: (error: AppError) => void }) {
  const [enabled, setEnabled] = useNsfwSetting();
  const [saving, setSaving] = useState(false);
  const pending = useRef(false);
  async function toggle() {
    if (enabled === null || pending.current) return;
    pending.current = true;
    setSaving(true);
    try {
      setEnabled(await callCommand<boolean>("set_nsfw_enabled", { enabled: !enabled }));
    } catch (error) {
      onError(normalizeAppError(error));
    } finally {
      pending.current = false;
      setSaving(false);
    }
  }
  const label = enabled ? "关闭 NSFW" : "开启 NSFW";
  return <button type="button" className="brand-icon-wrap" aria-label={label} title={label}
    aria-pressed={enabled ?? false} disabled={enabled === null || saving} onClick={() => void toggle()}>
    <img src="/asmr-icon.png" alt="" className="brand-icon-img" draggable={false} />
  </button>;
}
