/** Save timing is explicit before editing; status comes only from the owning request. */
export function SettingsSaveNote({ children, busy = false, error = "", dirty = false }: {
  children: React.ReactNode; busy?: boolean; error?: string; dirty?: boolean;
}) {
  return <p className="settings-save-note">
    <span>{children}</span>
    <strong aria-live="polite">{busy ? "正在处理…" : error ? "操作未成功，修改未确认保存" : dirty ? "有未保存的修改" : ""}</strong>
  </p>;
}
