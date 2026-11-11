import type { DeviceSyncStatus } from "../../lib/deviceSync";
import { SettingsDisclosure } from "./SettingsDisclosure";
export function SyncResultDetails({ status }: { status: DeviceSyncStatus }) {
  const details = status.details;
  if (!details?.summary?.length) return null;
  return <div className="sync-result-details">
    <p className="settings-field-hint">当前同步资料 · 本次接收包含其他设备的新增、更改和删除；待匹配记录已保留，尚未关联音源。</p>
    <div className="sync-summary-grid">{details.summary.map(row => <div key={row.kind}>
      <strong>{row.label}</strong><span className="tabular-nums">{row.total}</span>
      <small>本次接收 {row.received}{row.pending > 0 ? ` · 待匹配 ${row.pending}` : ""}</small>
    </div>)}</div>
    {status.pending > 0 && <SettingsDisclosure title={`待匹配内容 · ${status.pending}`}>
      <p className="settings-field-hint">记录已保留。导入相同音频或补全在线作品后，再次同步即可重新匹配；不同剪辑版本不自动合并。</p>
      {details.pendingItems.map(item => <div className="sync-pending-item" key={item.key}><strong>{item.label}</strong><small>{item.reason}</small></div>)}
      {status.pending > details.pendingItems.length && <p className="settings-field-hint">显示前 {details.pendingItems.length} 条；其余记录仍保留，包括歌单内缺失的音轨。</p>}
    </SettingsDisclosure>}
  </div>;
}
