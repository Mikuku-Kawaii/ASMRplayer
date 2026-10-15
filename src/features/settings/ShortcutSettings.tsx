import { useEffect, useId, useRef, useState } from "react";
import { ChevronDown, ChevronUp, Keyboard, RotateCcw } from "lucide-react";
import { commands, defaultBindings, eventBinding, formatBinding, validateBindings } from "../../lib/shortcuts";
import { normalizeAppError } from "../../lib/api";
import { useShortcuts } from "../commands/ShortcutProvider";

export function ShortcutSettings({ smtcMode }: { smtcMode?: string }) {
  const { bindings, ready, saving, loadError, save, reload } = useShortcuts();
  const [expanded, setExpanded] = useState(false);
  const contentId = useId();
  const [editing, setEditing] = useState<string | null>(null);
  const [candidate, setCandidate] = useState<string | null>(null);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [reset, setReset] = useState(false);
  const [filter, setFilter] = useState("");
  const editTrigger = useRef<HTMLButtonElement | null>(null);
  const busy = !ready || saving;
  useEffect(() => {
    if (!editing && !busy && editTrigger.current) { editTrigger.current.focus(); editTrigger.current = null; }
  }, [editing, busy]);
  async function commit(id: string, value: string | null) {
    setError(""); setNotice("");
    try { await save({ ...bindings, [id]: value }); setEditing(null); setNotice("快捷键已保存并生效。"); }
    catch (e) { setError(`未生效：${normalizeAppError(e).message}`); }
  }
  return <section className="settings-card settings-group-card shortcut-settings" data-shortcut-editor aria-label="快捷键设置" onKeyDown={event => event.stopPropagation()}>
    <div className="settings-group-header">
      <div className="settings-group-title-wrap">
        <span className="settings-group-icon"><Keyboard size={16} aria-hidden="true" /></span>
        <div><h3>快捷键与命令搜索</h3><p className="settings-group-desc">查看或修改播放、队列、字幕等操作的快捷键；不需要时可以收起，已保存的快捷键仍会生效。</p></div>
      </div>
      <button type="button" className="btn btn-ghost btn-sm" aria-expanded={expanded} aria-controls={contentId} onClick={() => {
        setExpanded(value => !value); setEditing(null); setCandidate(null); setReset(false); setError(""); editTrigger.current = null;
      }}>{expanded ? <ChevronUp size={14} aria-hidden="true" /> : <ChevronDown size={14} aria-hidden="true" />}<span className="ui-icon-label">{expanded ? "收起快捷键" : "展开快捷键"}</span></button>
    </div>
    <div id={contentId} hidden={!expanded}>
    {expanded && <div className="settings-group-body shortcut-settings-body">
    <div className="shortcut-actions"><button type="button" className="btn btn-ghost btn-sm" disabled={busy} onClick={() => { setReset(true); setError(""); setEditing(null); }}><RotateCcw size={14} aria-hidden="true" /><span className="ui-icon-label">恢复默认</span></button></div>
    <p className="settings-field-hint">应用内快捷键仅在主窗口有焦点时生效；输入框、弹窗及菜单编辑时保留正常输入。长按不会重复触发。</p>
    <label className="shortcut-search">查找操作<input className="input-text" value={filter} onChange={event => setFilter(event.target.value)} placeholder="播放、队列、字幕…" /></label>
    {loadError && <div className="shortcut-error" role="alert">{loadError}<button type="button" className="btn btn-ghost btn-sm" disabled={busy} onClick={reload}>重试读取</button></div>}
    {reset && <div className="shortcut-reset" role="group" aria-label="恢复默认快捷键">
      <p>恢复全部应用内快捷键？这会覆盖自定义组合和已清除的绑定。</p>
      <div className="shortcut-actions"><button autoFocus type="button" className="btn btn-ghost btn-sm" disabled={busy} onClick={() => setReset(false)}>取消</button>
        <button type="button" className="btn btn-primary btn-sm" disabled={busy} onClick={() => {
          setError(""); void save(defaultBindings).then(() => { setReset(false); setNotice("已恢复默认快捷键。"); }).catch(e => setError(`未生效：${normalizeAppError(e).message}`));
        }}>{saving ? "正在恢复…" : "确认恢复默认"}</button></div></div>}
    <div className="shortcut-rows">
      {commands.filter(command => `${command.label} ${command.keywords}`.toLowerCase().includes(filter.trim().toLowerCase())).map(command => <div className="shortcut-row" key={command.id}>
        <div className="shortcut-command"><strong>{command.label}</strong><span>应用内 · {bindings[command.id] ? "已绑定" : "未绑定"}</span></div>
        <button type="button" className="btn btn-secondary btn-sm shortcut-binding" disabled={busy || reset} aria-label={`编辑${command.label}快捷键`}
          onClick={event => { editTrigger.current = event.currentTarget; setEditing(command.id); setCandidate(bindings[command.id] ?? null); setError(""); setNotice(""); }}><kbd>{formatBinding(bindings[command.id])}</kbd></button>
        {editing === command.id && <div className="shortcut-editor">
          <label>按下新的组合键<input autoFocus readOnly className="input-text" aria-label={`录制${command.label}快捷键`} value={candidate ? formatBinding(candidate) : "按下组合键"} disabled={busy}
            onKeyDown={event => {
              if (event.key === "Tab") return;
              event.preventDefault(); event.stopPropagation();
              if (event.key === "Escape") { setEditing(null); setError(""); return; }
              if (event.repeat || event.nativeEvent.isComposing || event.keyCode === 229) return;
              if (event.metaKey || event.getModifierState("AltGraph")) { setError("未生效：Win / AltGr 组合由系统或输入法管理。"); return; }
              const next = eventBinding(event.nativeEvent); if (!next) return;
              setCandidate(next); setError(validateBindings({ ...bindings, [command.id]: next }) ?? "");
            }} /></label>
          <p className="settings-field-hint">Esc 取消，Tab 移动焦点。保存后生效；可清除绑定，继续通过按钮或命令搜索操作。</p>
          <div className="shortcut-actions"><button type="button" className="btn btn-ghost btn-sm" disabled={busy} onClick={() => { setEditing(null); setError(""); }}>取消</button>
            <button type="button" className="btn btn-ghost btn-sm" disabled={busy} onClick={() => void commit(command.id, null)}>清除绑定</button>
            <button type="button" className="btn btn-primary btn-sm" disabled={busy || !candidate || !!validateBindings({ ...bindings, [command.id]: candidate })} onClick={() => void commit(command.id, candidate)}>{saving ? "正在保存…" : "保存快捷键"}</button></div>
        </div>}
      </div>)}
      {!commands.some(command => `${command.label} ${command.keywords}`.toLowerCase().includes(filter.trim().toLowerCase())) && <p className="settings-field-hint">没有匹配操作，请换一个关键词。</p>}
    </div>
    {error && <p className="shortcut-error" role="alert">{error}</p>}
    {notice && <p className="settings-field-hint" role="status">{notice}</p>}
    <div className="shortcut-system"><strong>系统媒体键 · 全局</strong><p className="settings-field-hint">播放 / 暂停、上一首、下一首由 Windows 系统媒体控制（SMTC）接收，窗口在后台时也可使用，不在这里重绑定。实际响应由 Windows 媒体会话和设备决定。</p>
      <p className="settings-field-hint">{smtcMode === "disabled" ? "当前 SMTC 已关闭，系统媒体键不会控制本播放器。可在上方「内容安全、隐私防护与窗口行为」中开启系统媒体控制。" : smtcMode ? "当前 SMTC 已开启。" : "系统媒体控制状态尚未读取。"}</p></div>
    </div>}
    </div>
  </section>;
}
