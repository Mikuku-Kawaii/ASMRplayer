import { useRef, useState } from "react";
import { Select } from "../../components/Select";
import { AppError, callCommand, normalizeAppError, WorkDetailDto } from "../../lib/api";

export function WorkRatingSelect({ detail, onSaved, onError }: {
  detail: WorkDetailDto; onSaved: (detail: WorkDetailDto) => void; onError: (error: AppError) => void;
}) {
  const [saving, setSaving] = useState(false);
  const pending = useRef(false);
  const override = detail.overrides.find(item => item.field === "nsfw" && item.overrideMode === "set");
  const value = override?.valueText === "true" || override?.valueText === "false" ? override.valueText : "auto";
  async function save(next: string) {
    if (pending.current || next === value) return;
    pending.current = true; setSaving(true);
    try {
      const result = next === "auto"
        ? await callCommand<WorkDetailDto>("reset_work_override", { workId: detail.id, field: "nsfw" })
        : await callCommand<WorkDetailDto>("set_work_override", { workId: detail.id, field: "nsfw", overrideMode: "set", valueText: next });
      onSaved(result);
    } catch (error) { onError(normalizeAppError(error)); }
    finally { pending.current = false; setSaving(false); }
  }
  return <div className="override-row">
    <label htmlFor="work-r18-rating">是否为 R18</label>
    <Select id="work-r18-rating" size="compact" value={value} disabled={saving} onValueChange={next => void save(next)}
      options={[{ value: "auto", label: "自动识别" }, { value: "true", label: "是（R18）" }, { value: "false", label: "否（全年龄）" }]} />
  </div>;
}
