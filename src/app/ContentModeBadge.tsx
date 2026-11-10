import { useRef, useState } from "react";
import { AppError, callCommand, normalizeAppError } from "../lib/api";
import { useNsfwSetting } from "../features/settings/useNsfwSetting";

/** Select the host's saved content filter, not the current work's age rating. */
export function ContentModeBadge({ onError }: { onError: (error: AppError) => void }) {
  const [enabled, setEnabled] = useNsfwSetting();
  const [saving, setSaving] = useState(false);
  const pending = useRef(false);

  async function select(next: boolean) {
    if (enabled === null || next === enabled || pending.current) return;
    pending.current = true;
    setSaving(true);
    try {
      setEnabled(await callCommand<boolean>("set_nsfw_enabled", { enabled: next }));
    } catch (error) {
      onError(normalizeAppError(error));
    } finally {
      pending.current = false;
      setSaving(false);
    }
  }

  return <div className="content-mode-switch" role="group" aria-label="内容显示模式" aria-busy={saving}>
    {[false, true].map(mode => <button key={String(mode)} type="button"
      className={`content-mode-tag ${mode ? "is-nsfw" : "is-sfw"}`}
      aria-pressed={enabled === mode} disabled={enabled === null || saving}
      title={mode ? "切换为 NSFW：显示成人向内容" : "切换为 SFW：隐藏已识别的成人向内容"}
      onClick={() => void select(mode)}>
      <span className="content-mode-label">{mode ? "NSFW" : "SFW"}</span>
    </button>)}
  </div>;
}
