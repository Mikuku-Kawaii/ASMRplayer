import { Captions, RotateCcw } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { Select } from "../../components/Select";
import { callCommand, normalizeAppError } from "../../lib/api";
import { lyricsFonts } from "../lyrics/lyricsAppearance";
import { useLyricsSettings } from "../lyrics/useLyricsSettings";

function LyricsRange({ id, value, min, max, step, disabled, format, commit }: {
  id: string; value: number; min: number; max: number; step: number; disabled: boolean;
  format: (n: number) => string; commit: (n: number) => void;
}) {
  const [draft, setDraft] = useState(value);
  const dirty = useRef(false);
  useEffect(() => { if (!disabled) { setDraft(value); dirty.current = false; } }, [value, disabled]);
  const save = () => { if (dirty.current) { dirty.current = false; commit(draft); } };
  return <div className="lyrics-setting-control"><input id={id} type="range" min={min} max={max} step={step} value={draft} disabled={disabled}
    onChange={e => { dirty.current = true; setDraft(Number(e.target.value)); }}
    onPointerUp={save} onKeyUp={save} onBlur={save} onPointerCancel={() => { dirty.current = false; setDraft(value); }} />
    <output>{format(draft)}</output></div>;
}

export function DesktopLyricsSettings() {
  const { settings: s, update, busy, error, setError } = useLyricsSettings();
  return <section className="settings-card settings-group-card" aria-label="桌面歌词设置">
    <div className="settings-group-header"><div className="settings-group-title-wrap"><span className="settings-group-icon"><Captions size={18} /></span><div><h3>桌面歌词</h3><p className="settings-group-desc">独立置顶显示当前字幕；锁定后固定当前位置，右键可调整显示。</p></div></div></div>
    <div className="settings-group-body">
      {([['enabled','开启桌面歌词'],['locked','锁定当前位置'],['backgroundEnabled','显示歌词背景'],['showTranslation','显示双语字幕']] as const).map(([key,label]) => <div className="settings-row" key={key}><div className="settings-label-block"><label htmlFor={`lyrics-${key}`}>{label}</label>{key === 'locked' && <span className="settings-field-hint">锁定后不能拖动，可右键解锁。</span>}</div><input id={`lyrics-${key}`} type="checkbox" role="switch" className="toggle-switch" checked={s?.[key] ?? false} disabled={!s || busy} onChange={e => void update({ [key]: e.target.checked })} /></div>)}
      <div className="settings-row"><label htmlFor="lyrics-size">字幕字号</label><LyricsRange id="lyrics-size" min={20} max={64} step={2} value={s?.fontSize ?? 32} disabled={!s || busy} format={n => `${n} px`} commit={n => void update({ fontSize: n })} /></div>
      <div className="settings-row"><div className="settings-label-block"><label htmlFor="lyrics-font">字幕字体</label><span className="settings-field-hint">未安装的字体会自动使用备用字体。</span></div><Select id="lyrics-font" value={s?.fontFamily ?? "system"} disabled={!s || busy} options={lyricsFonts} onValueChange={fontFamily => void update({ fontFamily: fontFamily as NonNullable<typeof s>["fontFamily"] })} /></div>
      <div className="settings-row"><label htmlFor="lyrics-color">字幕颜色</label><Select id="lyrics-color" value={s?.color ?? "lavender"} disabled={!s || busy} options={[{ value:"lavender",label:"薰衣草紫" },{ value:"white",label:"柔白" },{ value:"amber",label:"暖金" }]} onValueChange={color => void update({ color: color as NonNullable<typeof s>["color"] })} /></div>
      <div className="settings-row"><label htmlFor="lyrics-opacity">背景不透明度</label><LyricsRange id="lyrics-opacity" min={0} max={0.8} step={0.05} value={s?.backgroundOpacity ?? 0.25} disabled={!s || busy || !s.backgroundEnabled} format={n => `${Math.round(n * 100)}%`} commit={n => void update({ backgroundOpacity: n })} /></div>
      <div className="settings-row"><span className="settings-field-hint">日文与中文按字幕原有内容显示；字幕间隔保留上一句。</span><button className="btn btn-secondary btn-sm" type="button" disabled={!s || busy || s.locked} onClick={() => void callCommand("desktop_lyrics_reset_position").then(() => setError("")).catch(e => setError(normalizeAppError(e).message))}><RotateCcw size={14} />恢复原位</button></div>
      {error && <p className="error-banner" role="alert">{error}</p>}
    </div>
  </section>;
}
