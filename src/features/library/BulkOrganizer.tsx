import { ReactNode, useEffect, useRef, useState } from "react";
import { BulkAction, BulkReport, PlaybackSnapshot, callCommand, normalizeAppError } from "../../lib/api";
import { ContextMenu } from "../../components/ContextMenu";

const statusText = { ready: "将修改", unchanged: "无需修改", failed: "未完成", applied: "已修改", undone: "已撤销", undoFailed: "撤销未完成" };
function describe(action: BulkAction) {
  switch (action.kind) {
    case "collected": return action.value ? "移到我的音频库" : "移出我的音频库（保留文件）";
    case "favorite": return action.value ? "加入喜欢" : "取消喜欢";
    case "listened": return action.value ? "标为听过" : "标为未听";
    case "addTags": return `添加标签：${action.tags.join("、")}`;
    case "removeTags": return `移除标签：${action.tags.join("、")}`;
  }
}
interface Props {
  context: { x: number; y: number; work: { id: string; title: string } | null } | null;
  onClose: () => void; children: ReactNode;
  selecting: boolean; onSelecting: (value: boolean) => void;
  selectedIds: string[]; onSelectVisible: () => void; visibleCount: number;
  onBusy: (value: boolean) => void; onChanged: () => void;
  onSnapshotChange?: (snapshot: PlaybackSnapshot) => void;
}
export function BulkOrganizer({ context, onClose, children, selecting, onSelecting, selectedIds, onSelectVisible, visibleCount, onBusy, onChanged, onSnapshotChange }: Props) {
  const [page, setPage] = useState<"actions" | "move" | "tags" | "report">("actions");
  const [tagKind, setTagKind] = useState<"addTags" | "removeTags">("addTags");
  const [tags, setTags] = useState("");
  const [report, setReport] = useState<BulkReport | null>(null);
  const [latest, setLatest] = useState<BulkReport | null>(null);
  const [error, setError] = useState("");
  const [pending, setPending] = useState(false);
  const busy = useRef(false);
  const latestEpoch = useRef(0);
  const selectionKey = JSON.stringify([...selectedIds].sort());
  const currentSelection = useRef(selectionKey); currentSelection.current = selectionKey;
  const previewSelection = useRef("");
  const menuEpoch = useRef(0);
  useEffect(() => { menuEpoch.current++; setPage("actions"); setError(""); }, [context]);
  useEffect(() => {
    if (!context) return;
    let alive = true;
    const epoch = latestEpoch.current;
    void callCommand<BulkReport | null>("latest_library_batch").then(value => {
      if (alive && epoch === latestEpoch.current) setLatest(value?.id ? value : null);
    }).catch(e => { if (alive && epoch === latestEpoch.current) setError(normalizeAppError(e).message); });
    return () => { alive = false; };
  }, [context]);
  const ready = report?.items.filter(item => item.status === "ready").length ?? 0;
  const previewValid = report?.phase === "preview" && previewSelection.current === selectionKey;
  const undoable = latest?.items.some(item => item.status === "applied" || item.status === "undoFailed");
  async function run(command: string, args?: Record<string, unknown>) {
    if (busy.current) return;
    busy.current = true; setPending(true); onBusy(true); setError("");
    const requestSelection = selectionKey, requestMenu = menuEpoch.current;
    if (command !== "preview_library_batch") latestEpoch.current++;
    try {
      const result = await callCommand<BulkReport>(command, args);
      if (command === "preview_library_batch") {
        if (currentSelection.current !== requestSelection || menuEpoch.current !== requestMenu) return;
        previewSelection.current = requestSelection;
      }
      setReport(result);
      if (menuEpoch.current === requestMenu) setPage("report");
      if (command !== "preview_library_batch") {
        onChanged();
        if (result.items.some(item => ["applied", "undone", "undoFailed"].includes(item.status))) setLatest(result);
        const newest = await callCommand<BulkReport | null>("latest_library_batch");
        setLatest(newest?.id ? newest : null);
      }
    } catch (e) { setError(normalizeAppError(e).message); }
    finally { busy.current = false; setPending(false); onBusy(false); }
  }
  const preview = (action: BulkAction) => void run("preview_library_batch", { workIds: selectedIds, action });
  async function addToQueue() {
    if (busy.current || !selectedIds.length) return;
    busy.current = true; setPending(true); onBusy(true); setError("");
    try {
      const snapshot = await callCommand<PlaybackSnapshot>("append_works_to_queue", { workIds: selectedIds });
      onSnapshotChange?.(snapshot);
      onClose();
    } catch (e) { setError(normalizeAppError(e).message); }
    finally { busy.current = false; setPending(false); onBusy(false); }
  }
  const item = (label: string, onClick: () => void, disabled = false) => <button type="button" className="m3-menu-item" role="menuitem" disabled={pending || disabled} onClick={onClick}>{label}</button>;
  if (!context) return null;
  return <ContextMenu x={context.x} y={context.y} onClose={onClose}>
    <div className="m3-context-menu-header">
      <div className="m3-context-menu-title">{selecting ? `已选 ${selectedIds.length} 部作品` : context.work?.title ?? "作品库"}</div>
    </div>
    {page === "actions" && <>
      {selecting ? <>
        {item("加入队列", () => void addToQueue(), !selectedIds.length)}
        {item("移动到…", () => setPage("move"), !selectedIds.length)}
        {item("移出我的音频库（保留文件）", () => preview({ kind: "collected", value: false }), !selectedIds.length)}
        {item("取消喜欢", () => preview({ kind: "favorite", value: false }), !selectedIds.length)}
        {item("添加标签…", () => { setTagKind("addTags"); setTags(""); setPage("tags"); }, !selectedIds.length)}
        {item("移除标签…", () => { setTagKind("removeTags"); setTags(""); setPage("tags"); }, !selectedIds.length)}
        {item("标为听过", () => preview({ kind: "listened", value: true }), !selectedIds.length)}
        {item("标为未听", () => preview({ kind: "listened", value: false }), !selectedIds.length)}
        <div className="m3-menu-divider" />
        {item("全选已显示作品", () => { onSelectVisible(); onClose(); }, !visibleCount)}
        {item("退出多选", () => { onSelecting(false); onClose(); })}
      </> : <>
        {item("多选", () => { onSelecting(true); onClose(); }, !visibleCount)}
        {children}
      </>}
      {latest && undoable && <><div className="m3-menu-divider" />{item("撤销最近一次批量修改", () => void run("undo_library_batch", { id: latest.id }))}</>}
      {(report || latest) && item("查看最近处理结果", () => { if (!report) setReport(latest); setPage("report"); })}
    </>}
    {page === "move" && <>
      {item("返回操作菜单", () => setPage("actions"))}
      <div className="m3-menu-divider" />
      {item("我的音频库", () => preview({ kind: "collected", value: true }), !selectedIds.length)}
      {item("喜欢", () => preview({ kind: "favorite", value: true }), !selectedIds.length)}
    </>}
    {page === "tags" && <>
      {item("返回操作菜单", () => setPage("actions"))}
      <label className="bulk-menu-field">{tagKind === "addTags" ? "添加标签" : "移除标签"}<input className="input-text" value={tags} disabled={pending} maxLength={2048} placeholder="用逗号分隔，最多 16 项" onChange={e => setTags(e.target.value)} /></label>
      {item("预览影响", () => preview({ kind: tagKind, tags: tags.split(/[,，、\n]/).map(t => t.trim()).filter(Boolean) }), !tags.trim() || !selectedIds.length)}
    </>}
    {page === "report" && report && <>
      <div className="bulk-menu-report">
        <strong>{describe(report.action)}</strong>
        <p role="status">{report.phase === "preview" ? `将修改 ${ready} 部作品` : `已修改 ${report.items.filter(i => i.status === "applied").length} 部，已撤销 ${report.items.filter(i => i.status === "undone").length} 部`}，无需修改 {report.items.filter(i => i.status === "unchanged").length} 部，未完成 {report.items.filter(i => ["failed", "undoFailed"].includes(i.status)).length} 部。</p>
        <ul className="bulk-result-list" aria-label="逐项处理结果">{report.items.map(entry => <li key={entry.workId}>
          <span>{entry.title}</span><strong>{statusText[entry.status]}</strong>{entry.error && <small>{entry.error}</small>}
          {report.phase === "applied" && entry.status === "failed" && item("重试此项", () => void run("execute_library_batch", { id: report.id, retryIds: [entry.workId] }))}
        </li>)}</ul>
        {report.phase === "preview" && <p>{previewValid ? "只修改列表引用或整理标记，保留媒体文件与播放进度。" : "选择已变化，请返回菜单重新预览。"}</p>}
      </div>
      {report.phase === "preview" && item(`确认修改 ${ready} 部作品`, () => void run("execute_library_batch", { id: report.id }), !previewValid || !ready)}
      {item("返回操作菜单", () => setPage("actions"))}
    </>}
    {pending && <p className="bulk-menu-message" role="status">正在处理…</p>}
    {error && <p role="alert" className="bulk-menu-message">{error}</p>}
  </ContextMenu>;
}
