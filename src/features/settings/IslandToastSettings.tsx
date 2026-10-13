import { useEffect, useRef, useState, type CSSProperties } from "react";

interface Props {
  enabled: boolean;
  duration: number;
  onToggle: (enabled: boolean) => void;
  onDurationCommit: (seconds: number) => void;
}

export function IslandToastSettings({ enabled, duration, onToggle, onDurationCommit }: Props) {
  const [draft, setDraft] = useState(duration);
  const committed = useRef(duration);
  useEffect(() => { setDraft(duration); committed.current = duration; }, [duration]);
  const commit = () => {
    if (draft === committed.current) return;
    committed.current = draft;
    onDurationCommit(draft);
  };
  return <div className="settings-row island-notification-setting">
    <div className="settings-label-block">
      <span id="island-toast-label">切歌与音量通知</span>
      <span className="settings-field-hint" id="island-toast-hint">短暂显示曲目信息或音量，随后恢复字幕。</span>
    </div>
    <div className="island-notification-controls">
      <div className="island-notification-heading">
        <label htmlFor="setting-island-toast-sec">停留时长</label>
        <button type="button" className="settings-switch" role="switch" aria-checked={enabled}
          aria-labelledby="island-toast-label" aria-describedby="island-toast-hint"
          title={enabled ? "关闭切歌与音量通知" : "开启切歌与音量通知"} onClick={() => onToggle(!enabled)}>
          <span aria-hidden="true" />
        </button>
      </div>
      <div className="island-duration-control" data-disabled={!enabled}>
        <input id="setting-island-toast-sec" type="range" min={2} max={5} step={0.5}
          value={draft} disabled={!enabled} aria-valuetext={`${draft.toFixed(1)} 秒`}
          style={{ "--range-progress": `${(draft - 2) / 3 * 100}%` } as CSSProperties}
          onChange={e => setDraft(Number(e.target.value))} onPointerUp={commit} onKeyUp={commit} onBlur={commit}
          onPointerCancel={() => setDraft(committed.current)} />
        <output htmlFor="setting-island-toast-sec" className="tabular-nums">{draft.toFixed(1)}<span>秒</span></output>
      </div>
      <div className="island-duration-endpoints" aria-hidden="true"><span>2 秒</span><span>5 秒</span></div>
    </div>
  </div>;
}
