import { SettingsSaveNote } from "./SettingsSaveNote";
import { themePresets, useTheme, validTheme } from "../../app/ThemeProvider";
import { Select } from "../../components/Select";
import { HelpHint } from "../../components/HelpHint";
import { ErrorNotice } from "../../components/ErrorNotice";

export function ThemeSettings() {
  const { preset, ready, pending, error, save } = useTheme();
  const inkBlue = preset.startsWith("ink_blue");
  const selected = themePresets.find(item => item.value === (inkBlue ? "ink_blue" : preset))!;
  return <>
    <SettingsSaveNote busy={pending} error={error}>自动保存 · 立即更新所有窗口的主题</SettingsSaveNote>
    <div className="settings-row">
      <span className="setting-label-help"><label htmlFor="theme-preset">主题预设</label><HelpHint label="主题预设" text="选择后自动保存并立即生效，默认藤紫。琥珀烛光采用暖暗色调，适合关灯后的低亮度环境。灵动岛播放条和波动条仍跟随封面颜色。" /></span>
      <div className="theme-preset-control">
        <div className="theme-preset-swatches" aria-hidden="true">
          {selected.colors.map(color => <span key={color} style={{ backgroundColor: color }} />)}
        </div>
        <Select id="theme-preset" value={selected.value} options={themePresets} disabled={!ready || pending}
          onValueChange={value => { if (validTheme(value)) void save(value); }} />
      </div>
    </div>
    {inkBlue && <div className="settings-row">
      <label htmlFor="ink-blue-mode">明暗模式</label>
      <Select id="ink-blue-mode" value={preset} disabled={!ready || pending}
        options={[{value:"ink_blue", label:"深色"}, {value:"ink_blue_light", label:"浅色"}, {value:"ink_blue_system", label:"跟随系统"}]}
        onValueChange={value => { if (validTheme(value)) void save(value); }} />
    </div>}
    {pending && <p className="text-secondary" role="status">正在保存主题…</p>}
    {error && <ErrorNotice message={error} />}
  </>;
}
