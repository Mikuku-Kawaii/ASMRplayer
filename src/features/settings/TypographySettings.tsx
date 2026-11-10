import { useEffect, useState } from "react";
import { fontPresets, useTypography } from "../../app/TypographyProvider";
import { Select } from "../../components/Select";
import { HelpHint } from "../../components/HelpHint";
import { ErrorNotice } from "../../components/ErrorNotice";

export function TypographySettings() {
  const { value, save, error } = useTypography();
  const [available, setAvailable] = useState<boolean | null>(null);
  useEffect(() => {
    let disposed = false;
    setAvailable(null);
    if (value.font === "noto" && document.fonts) {
      void document.fonts.load('500 14px "ASMR Noto Sans SC"').then(
        faces => { if (!disposed) setAvailable(faces.length > 0); },
        () => { if (!disposed) setAvailable(false); },
      );
    } else if (value.font !== "noto" && typeof FontFace !== "undefined") {
      const preset = fontPresets.find(item => item.value === value.font)!;
      void Promise.all(preset.localNames.map(name => new FontFace("availability-probe", `local("${name}")`).load()
        .then(() => true, () => false))).then(results => { if (!disposed) setAvailable(results.some(Boolean)); });
    }
    return () => { disposed = true; };
  }, [value.font]);
  return <>
    <div className="settings-row">
      <span className="setting-label-help"><label htmlFor="interface-font">界面字体</label><HelpHint label="界面字体" text="选择即保存到当前设备，并同步主窗口与灵动岛。Noto Sans SC 已内置，无需安装或联网。微软雅黑、等线使用本机字体，未安装时回退到内置字体。桌面歌词选择“跟随界面”时同步，其他独立字体选择不变。" /></span>
      <Select id="interface-font" value={value.font} options={fontPresets} onValueChange={font => {
        const preset = fontPresets.find(item => item.value === font);
        if (preset) save({ ...value, font: preset.value });
      }} />
    </div>
    <div className="settings-row">
      <span className="setting-label-help"><label htmlFor="interface-font-weight">正文粗细</label><HelpHint label="正文粗细" text="默认中等，标题继续使用较粗字重。部分系统字体没有中等字重，会选用最接近的可用字重。灵动岛与桌面歌词保留各自的粗细设置。" /></span>
      <Select id="interface-font-weight" value={String(value.weight)} options={[
        { value: "400", label: "标准" }, { value: "500", label: "中等（默认）" }, { value: "600", label: "加粗" },
      ]} onValueChange={weight => { const next = Number(weight); if (next === 400 || next === 500 || next === 600) save({ ...value, weight: next }); }} />
    </div>
    <div className="typography-preview" aria-label="字体预览">
      <p>播放与设置 · 简中字形：骨、直、令、门、语</p>
      <p>おやすみなさい。君に会えてよかった。</p>
      <p className="tabular-nums">ASMR Player · 0123456789 · 08:24 / 32:16</p>
    </div>
    {available === false && <p className="text-secondary" role="status">{value.font === "noto" ? "内置字体加载失败，当前使用系统备用字体。" : "此字体未安装，当前使用备用简中字体。"}</p>}
    {error && <ErrorNotice message={error} />}
  </>;
}
