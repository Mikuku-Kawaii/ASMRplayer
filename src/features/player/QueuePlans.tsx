import { useEffect, useId, useRef, useState } from "react";
import { ListMusic, Pencil, Plus, RefreshCw, Save } from "lucide-react";
import { Select } from "../../components/Select";
import { callCommand, normalizeAppError, type PlaybackSnapshot, type QueuePlanDto, type QueuePlanPreviewDto } from "../../lib/api";

export function QueuePlans({ snapshot, onSnapshotChange }: {
  snapshot: PlaybackSnapshot; onSnapshotChange: (snapshot: PlaybackSnapshot) => void;
}) {
  const id = useId();
  const [plans, setPlans] = useState<QueuePlanDto[]>([]);
  const [selected, setSelected] = useState("");
  const [draft, setDraft] = useState("");
  const [rename, setRename] = useState<string | null>(null);
  const [preview, setPreview] = useState<QueuePlanPreviewDto | null>(null);
  const [skip, setSkip] = useState(false);
  const [confirm, setConfirm] = useState(false);
  const [busy, setBusy] = useState(false);
  const [loading, setLoading] = useState(true);
  const [refresh, setRefresh] = useState(0);
  const [error, setError] = useState("");
  const [message, setMessage] = useState("");
  const [limit, setLimit] = useState(50);
  const locked = useRef(false);
  const mounted = useRef(false);
  const replaceButton = useRef<HTMLButtonElement>(null);
  const cancelButton = useRef<HTMLButtonElement>(null);
  const queueIdentity = snapshot.queue.map(item => `${item.id}:${item.preferredSourceId ?? ""}`).join("|");

  useEffect(() => {
    mounted.current = true;
    return () => { mounted.current = false; };
  }, []);
  useEffect(() => {
    let active = true;
    setLoading(true);
    setError("");
    callCommand<QueuePlanDto[]>("list_queue_plans").then(data => { if (active) setPlans(data); })
      .catch(e => { if (active) setError(normalizeAppError(e).message); })
      .finally(() => { if (active) setLoading(false); });
    return () => { active = false; };
  }, [refresh]);
  useEffect(() => {
    let active = true;
    setPreview(null); setSkip(false); setConfirm(false); setRename(null); setLimit(50);
    if (selected) {
      setError("");
      callCommand<QueuePlanPreviewDto>("preview_queue_plan", { planId: selected })
        .then(data => { if (active) setPreview(data); })
        .catch(e => { if (active) setError(normalizeAppError(e).message); });
    }
    return () => { active = false; };
  }, [selected, refresh, queueIdentity]);
  useEffect(() => { if (confirm) cancelButton.current?.focus(); }, [confirm]);

  async function run(action: () => Promise<void>) {
    if (locked.current) return;
    locked.current = true; setBusy(true); setError(""); setMessage("");
    try { await action(); } catch (e) { if (mounted.current) setError(normalizeAppError(e).message); }
    finally { locked.current = false; if (mounted.current) setBusy(false); }
  }
  const unavailable = preview?.items.filter(item => item.unavailableReason).length ?? 0;
  const available = (preview?.items.length ?? 0) - unavailable;
  const canApply = !!preview && available > 0 && (unavailable === 0 || skip) && !busy;
  async function apply(mode: "append" | "replace") {
    if (!preview || !canApply) return;
    await run(async () => {
      try {
        const next = await callCommand<PlaybackSnapshot>("apply_queue_plan", { request: {
          planId: preview.plan.id, mode, skipUnavailable: skip,
          previewToken: preview.previewToken, queueToken: preview.queueToken,
        } });
        if (!mounted.current) return;
        onSnapshotChange(next);
        setMessage(`已${mode === "append" ? "追加" : "恢复"} ${available} 项${unavailable ? `，跳过 ${unavailable} 个不可用项` : ""}。`);
        setRefresh(value => value + 1);
      } catch (e) {
        // Any retry must use a fresh preview and a new replacement confirmation.
        setPreview(null);
        throw e;
      } finally {
        setConfirm(false);
        replaceButton.current?.focus();
      }
    });
  }
  return <section className="queue-plans" aria-label="队列方案" onKeyDown={event => event.stopPropagation()}>
    <h3><ListMusic size={17} aria-hidden="true" />队列方案</h3>
    <form aria-label="保存当前队列" className="queue-plan-form" onSubmit={event => {
      event.preventDefault();
      void run(async () => {
        const plan = await callCommand<QueuePlanDto>("save_queue_plan", { name: draft.trim() });
        if (!mounted.current) return;
        setDraft(""); setSelected(plan.id); setRefresh(value => value + 1); setMessage(`已保存「${plan.name}」。`);
      });
    }}>
      <label htmlFor={`${id}-name`}>将当前队列保存为方案</label>
      <div className="queue-plan-row">
        <input className="input-text" id={`${id}-name`} maxLength={80} value={draft} placeholder="例如：睡前雨声" disabled={busy}
          onChange={event => setDraft(event.target.value)} />
        <button className="btn btn-tonal btn-sm" type="submit" disabled={busy || !draft.trim() || snapshot.queue.length === 0}><Save size={14} />保存</button>
      </div>
    </form>
    {snapshot.queue.length === 0 && <p className="queue-plan-hint">队列为空，加入音轨后即可保存。</p>}
    <div className="queue-plan-row">
      <Select aria-label="选择队列方案" size="compact" value={selected} disabled={busy || loading} placeholder={plans.length ? "选择方案以预览" : "暂无保存的方案"}
        options={plans.map(plan => ({ value: plan.id, label: `${plan.name} · ${plan.itemCount} 项` }))}
        onValueChange={value => { setSelected(value); setMessage(""); }} />
      <button type="button" className="btn btn-ghost btn-sm" aria-label="刷新队列方案" title="刷新队列方案" disabled={busy || loading} onClick={() => setRefresh(value => value + 1)}><RefreshCw size={14} /></button>
    </div>
    {selected && !preview && !error && <p className="queue-plan-hint" role="status">正在读取方案…</p>}
    {preview && <div className="queue-plan-preview" aria-label="方案预览">
      <div className="queue-plan-row"><strong>{preview.plan.name}</strong>
        <button type="button" className="btn btn-ghost btn-sm" disabled={busy || confirm} onClick={() => setRename(preview.plan.name)}><Pencil size={13} />重命名</button></div>
      {rename !== null && <form className="queue-plan-form" aria-label="重命名方案" onSubmit={event => {
        event.preventDefault();
        void run(async () => {
          await callCommand("rename_queue_plan", { planId: preview.plan.id, name: rename.trim() });
          if (mounted.current) { setRefresh(value => value + 1); setMessage("方案已重命名。"); }
        });
      }}>
        <label htmlFor={`${id}-rename`}>新方案名称</label>
        <input autoFocus id={`${id}-rename`} className="input-text" value={rename} maxLength={80} disabled={busy} onChange={event => setRename(event.target.value)} />
        <div className="queue-plan-actions"><button className="btn btn-ghost btn-sm" type="button" disabled={busy} onClick={() => setRename(null)}>取消</button>
          <button className="btn btn-tonal btn-sm" disabled={busy || !rename.trim()}>保存名称</button></div>
      </form>}
      <p className="queue-plan-hint">{available} 项可用 · {unavailable} 项不可用。按保存顺序恢复原版本，保留重复项。</p>
      <ol className="queue-plan-items" aria-label="方案音轨" tabIndex={0}>
        {preview.items.slice(0, limit).map((item, index) => <li key={`${index}-${item.versionId}`} data-unavailable={!!item.unavailableReason}>
          <span className="queue-plan-index">{index + 1}</span><div><strong>{item.trackTitle}</strong><span>{item.workTitle} · {item.versionLabel}</span>
            {item.unavailableReason && <em>{item.unavailableReason}</em>}</div></li>)}
        {preview.items.length > limit && <li><button type="button" className="btn btn-ghost btn-sm" onClick={() => setLimit(value => value + 100)}>展开更多（{limit}/{preview.items.length}）</button></li>}
      </ol>
      <p className="queue-plan-hint">在线来源的网络状态将在实际播放时检查。</p>
      {unavailable > 0 && <button type="button" className={`btn btn-sm ${skip ? "btn-tonal" : "btn-ghost"}`} aria-pressed={skip} disabled={busy || confirm} onClick={() => setSkip(value => !value)}>跳过 {unavailable} 个不可用项</button>}
      <div className="queue-plan-actions">
        <button type="button" className="btn btn-tonal btn-sm" disabled={!canApply || confirm} onClick={() => void apply("append")}><Plus size={14} />追加到队尾</button>
        <button type="button" ref={replaceButton} className="btn btn-ghost btn-sm" disabled={!canApply || confirm} onClick={() => setConfirm(true)}>替换当前队列</button>
      </div>
      {confirm && <div className="queue-plan-confirm" role="group" aria-labelledby={`${id}-confirm`} onKeyDown={event => {
        if (event.key === "Escape" && !busy) { event.preventDefault(); setConfirm(false); replaceButton.current?.focus(); }
      }}>
        <strong id={`${id}-confirm`}>确认替换当前队列？</strong>
        <p>当前 {preview.currentQueueCount} 项将替换为方案中的 {available} 项{unavailable ? `，跳过 ${unavailable} 个不可用项` : ""}。当前音频继续播放，不会删除音频文件。</p>
        <p>若当前版本在方案中，将从其首个位置继续；否则下一首从方案开头播放。单曲循环与睡眠定时仍按原设置生效。</p>
        <div className="queue-plan-actions"><button ref={cancelButton} className="btn btn-ghost btn-sm" type="button" disabled={busy} onClick={() => { setConfirm(false); replaceButton.current?.focus(); }}>取消</button>
          <button className="btn btn-primary btn-sm" type="button" disabled={!canApply} onClick={() => void apply("replace")}>{busy ? "正在替换…" : "确认替换"}</button></div>
      </div>}
    </div>}
    {error && <p className="queue-plan-error" role="alert">{error}</p>}
    {message && <p className="queue-plan-hint" role="status">{message}</p>}
  </section>;
}
