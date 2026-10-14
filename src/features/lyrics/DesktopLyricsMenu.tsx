import { useEffect } from "react";
import { Captions, Eye, EyeOff, Lock, Unlock, Minus, Plus, RotateCcw, X } from "lucide-react";
import { ContextMenu } from "../../components/ContextMenu";
import { callCommand, normalizeAppError, type DesktopLyricsSettingsDto } from "../../lib/api";
import { useLyricsSettings } from "./useLyricsSettings";

export function DesktopLyricsMenu() {
  const { settings, update, busy, error, setError } = useLyricsSettings();
  const close = () => { void callCommand("desktop_lyrics_close_menu").catch(e => setError(normalizeAppError(e).message)); };
  useEffect(() => {
    const focus = () => { setError(""); document.querySelector<HTMLElement>('[role="menu"]')?.focus(); };
    window.addEventListener("focus", focus);
    return () => window.removeEventListener("focus", focus);
  }, [setError]);
  async function change(patch: Partial<Omit<DesktopLyricsSettingsDto, "revision">>) {
    if (await update(patch)) close();
  }
  if (!settings) return error ? <p role="alert">{error}</p> : null;
  return <ContextMenu x={8} y={8} onClose={close} label="桌面歌词菜单">
    <div className="m3-context-menu-header"><span className="m3-context-menu-title">桌面歌词</span></div>
    <button className="m3-menu-item" role="menuitem" disabled={busy} onClick={() => void change({ locked: !settings.locked })}>{settings.locked ? <Unlock size={16} /> : <Lock size={16} />}{settings.locked ? "解锁位置" : "锁定当前位置"}</button>
    <div className="m3-menu-divider" role="separator" />
    <button className="m3-menu-item" role="menuitem" disabled={busy || settings.fontSize >= 64} onClick={() => void change({ fontSize: Math.min(64, settings.fontSize + 2) })}><Plus size={16} />放大字幕</button>
    <button className="m3-menu-item" role="menuitem" disabled={busy || settings.fontSize <= 20} onClick={() => void change({ fontSize: Math.max(20, settings.fontSize - 2) })}><Minus size={16} />缩小字幕</button>
    <button className="m3-menu-item" role="menuitem" disabled={busy} onClick={() => void change({ backgroundEnabled: !settings.backgroundEnabled })}>{settings.backgroundEnabled ? <EyeOff size={16} /> : <Eye size={16} />}{settings.backgroundEnabled ? "关闭背景" : "打开背景"}</button>
    <button className="m3-menu-item" role="menuitem" disabled={busy} onClick={() => void change({ showTranslation: !settings.showTranslation })}><Captions size={16} />{settings.showTranslation ? "隐藏双语字幕" : "显示双语字幕"}</button>
    <button className="m3-menu-item" role="menuitem" disabled={busy || settings.locked} onClick={() => void callCommand("desktop_lyrics_reset_position").then(close).catch(e => setError(normalizeAppError(e).message))}><RotateCcw size={16} />恢复原位</button>
    <div className="m3-menu-divider" role="separator" />
    <button className="m3-menu-item" role="menuitem" disabled={busy} onClick={() => void change({ enabled: false })}><X size={16} />关闭桌面歌词</button>
    {error && <p className="error-banner" role="alert">{error}</p>}
  </ContextMenu>;
}
