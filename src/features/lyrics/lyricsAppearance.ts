import type { DesktopLyricsSettingsDto } from "../../lib/api";

export const lyricsFonts: { value: DesktopLyricsSettingsDto["fontFamily"]; label: string; family: string }[] = [
  { value: "system", label: "跟随界面", family: "var(--font-interface)" },
  { value: "yahei", label: "微软雅黑", family: '"Microsoft YaHei UI", "Microsoft YaHei", sans-serif' },
  { value: "dengxian", label: "等线", family: 'DengXian, "Microsoft YaHei", sans-serif' },
  { value: "simsun", label: "宋体", family: 'SimSun, "Yu Mincho", serif' },
  { value: "kaiti", label: "楷体", family: 'KaiTi, "Microsoft YaHei", serif' },
  { value: "meiryo", label: "Meiryo（日文）", family: 'Meiryo, "Yu Gothic", "Microsoft YaHei", sans-serif' },
];
