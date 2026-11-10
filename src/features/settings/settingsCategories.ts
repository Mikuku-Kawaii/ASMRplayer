import { AppWindow, Cpu, FolderOpen, Globe, Languages, Palette, RefreshCw, ShieldAlert, Volume2 } from "lucide-react";

export const settingsSections = [
  { id: "appearance", members: ["appearance"], title: "外观与窗口", group: "使用偏好", icon: Palette },
  { id: "playback", members: ["audio-engine", "shortcuts"], title: "播放与快捷键", group: "使用偏好", icon: Volume2 },
  { id: "desktop", members: ["desktop-island", "desktop-lyrics"], title: "桌面显示", group: "使用偏好", icon: AppWindow },
  { id: "recognition", members: ["recognition"], title: "字幕识别", group: "使用偏好", icon: Cpu },
  { id: "translation", members: ["translation"], title: "字幕翻译", group: "使用偏好", icon: Languages },
  { id: "library", members: ["storage-library", "download-policy", "storage-media"], title: "媒体库与下载", group: "资源与系统", icon: FolderOpen },
  { id: "network", members: ["network-online"], title: "网络与账户", group: "资源与系统", icon: Globe },
  { id: "sync", members: ["device-sync"], title: "跨设备同步", group: "资源与系统", icon: RefreshCw },
  { id: "privacy", members: ["privacy"], title: "内容与隐私", group: "资源与系统", icon: ShieldAlert },
  { id: "maintenance", members: ["updates", "storage-cache", "backup-sync", "cache-diagnostics"], title: "数据与维护", group: "资源与系统", icon: RefreshCw },
];

const legacySections: Record<string, string> = {
  "privacy-window": "appearance", "recognition-translation": "recognition", storage: "library",
};
/** Existing app shortcuts and persisted sections remain valid after regrouping. */
export function resolveSettingsSection(id: string) {
  return settingsSections.find(item => item.id === id || item.members.includes(id))?.id ?? legacySections[id] ?? "appearance";
}
