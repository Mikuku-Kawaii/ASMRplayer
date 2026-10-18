import { useEffect, useRef, useState } from "react";
import { X } from "lucide-react";
import { ContentDialog } from "../../components/ContentDialog";
import { ErrorNotice } from "../../components/ErrorNotice";
import { callCommand, formatDurationMs, normalizeAppError } from "../../lib/api";

interface Candidate {
  sourceVersionId: string;
  targetVersionId: string;
  sourceLabel: string;
  targetLabel: string;
  sourceDurationMs: number;
  targetDurationMs: number;
  sourceKey: string;
  targetKey: string;
  revision: string;
}
export function SubtitleSyncDialog({ versionIds, onClose }: { versionIds: string[]; onClose: () => void }) {
  const [candidates, setCandidates] = useState<Candidate[]>([]);
  const [selected, setSelected] = useState<string[]>([]);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [synced, setSynced] = useState<number | null>(null);
  const locked = useRef(false);
  const key = JSON.stringify(versionIds);
  useEffect(() => {
    let active = true;
    setLoading(true); setError(""); setCandidates([]); setSelected([]); setSynced(null);
    void callCommand<Candidate[]>("preview_subtitle_sync", { versionIds: JSON.parse(key) }).then(rows => {
      if (active) { setCandidates(rows); setSelected(rows.map(row => row.targetVersionId)); }
    }).catch(e => { if (active) setError(normalizeAppError(e).message); }).finally(() => { if (active) setLoading(false); });
    return () => { active = false; };
  }, [key]);
  async function sync() {
    if (locked.current) return;
    locked.current = true; setBusy(true); setError("");
    try {
      const count = await callCommand<number>("sync_recognized_subtitles", { candidates: candidates.filter(row => selected.includes(row.targetVersionId)) });
      setSynced(count);
      window.dispatchEvent(new Event("subtitle-models-changed"));
    } catch (e) { setError(normalizeAppError(e).message); }
    finally { locked.current = false; setBusy(false); }
  }
  return <ContentDialog size="wide" label="同步识别字幕" busy={busy} onClose={onClose}>
    <div className="subtitle-document-dialog">
      <header className="library-remove-header"><h3>同步识别字幕</h3><button type="button" className="btn-icon" aria-label="关闭字幕同步" title="关闭字幕同步 (Esc)" disabled={busy} onClick={onClose}><X size={18} /></button></header>
      <p>将完整识别的原文、已保存译文和手工修正复制到同一作品的其他音频格式。请确认所选文件内容和时间轴相同。</p>
      <p className="settings-field-hint">按文件名、目录与时长寻找候选，仅忽略 WAV、MP3 等格式目录。已有字幕或识别记录的目标会跳过，播放进度保持独立。</p>
      {loading ? <p role="status">正在匹配音轨…</p> : synced !== null ? <p role="status">已同步到 {synced} 条音轨</p> : candidates.length ? <div className="subtitle-sync-candidates">
        {candidates.map(row => <label key={row.targetVersionId} className="subtitle-sync-candidate">
          <input type="checkbox" disabled={busy} checked={selected.includes(row.targetVersionId)} onChange={e => setSelected(ids => e.target.checked ? [...ids, row.targetVersionId] : ids.filter(id => id !== row.targetVersionId))} />
          <span><span>{row.sourceLabel} ({formatDurationMs(row.sourceDurationMs)})</span><span>→ {row.targetLabel} ({formatDurationMs(row.targetDurationMs)})</span></span>
        </label>)}
      </div> : !error && <p role="status">没有可同步的其他格式。请先完成源音轨识别，并导入同名、时长一致且尚无字幕的其他格式文件。</p>}
      {error && <ErrorNotice message={error} />}
      <div className="dialog-actions">
        <button type="button" className="btn btn-ghost" disabled={busy} onClick={onClose}>{synced !== null ? "关闭" : "取消"}</button>
        {synced === null && <button type="button" className="btn btn-primary" disabled={loading || busy || selected.length === 0} onClick={() => void sync()}>{busy ? "正在同步…" : `确认同步 (${selected.length})`}</button>}
      </div>
    </div>
  </ContentDialog>;
}
