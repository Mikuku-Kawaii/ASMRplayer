import { SettingsDisclosure } from "./SettingsDisclosure";
import { HelpHint } from "../../components/HelpHint";
import { DesktopLyricsSettings } from "./DesktopLyricsSettings";
import { usePageScroll } from "../../app/usePageScroll";
import { InterfaceScaleSettings } from "./InterfaceScaleSettings";
import { EngineMaintenanceSettings } from "./EngineMaintenanceSettings";
import { Select } from "../../components/Select";
import { Toast } from "../../components/Toast";
import { RemoveLibraryRootButton } from "./RemoveLibraryRootButton";
import React, { useEffect, useState, useCallback, useRef } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import {
  Search,
  FolderOpen,
  HardDrive,
  RefreshCw,
  Terminal,
  CheckCircle2,
  AlertTriangle,
  Cpu,
  ShieldAlert,
  Globe,
  EyeOff,
  Eye,
  AppWindow,
  Magnet,
  Pin,
  Volume2,
} from "lucide-react";
import {
  AppSettingsDto,
  LibraryRootDto,
  ScanReportDto,
  callCommand,
  normalizeAppError,
  AppError,
  subscribeSettingsUpdated,
} from "../../lib/api";
import { UpdateSettings } from "./UpdateSettings";
import { IslandToastSettings } from "./IslandToastSettings";
import { IslandDisplaySettings } from "./IslandDisplaySettings";
import { ShortcutSettings } from "./ShortcutSettings";
import { StorageSettings } from "./StorageSettings";
import { BackupSettings } from "./BackupSettings";
import { DiagnosticSettings } from "./DiagnosticSettings";
import { RecognitionSettings } from "./RecognitionSettings";

interface SettingsViewProps {
  onError: (err: AppError) => void;
  initialSection?: string;
  onSectionChange?: (section: string) => void;
  searchQuery?: string;
  onSearchQueryChange?: (value: string) => void;
}

const settingsSections = [
  { id:"privacy-window", members:["privacy-window", "shortcuts"], title:"通用与快捷键", hint:"隐私、窗口与键位", icon:ShieldAlert, keywords:"全年龄 关闭 托盘 NSFW SMTC 键盘 热键 命令 搜索 界面 缩放 大小 DPI 比例" },
  { id:"desktop-island", members:["desktop-island", "desktop-lyrics"], title:"桌面显示", hint:"灵动岛与桌面歌词", icon:AppWindow, keywords:"大小 屏幕 比例 吸附 全屏 音量 提示 字幕 字号 字重 粗细 双语 日文 中文 锁定 穿透 字体 颜色 透明度" },
  { id:"audio-engine", members:["audio-engine"], title:"音频与播放", hint:"输出与硬件加速", icon:Volume2, keywords:"独占 引擎 GPU 定时 重启 空闲 音轨 性能" },
  { id:"recognition-translation", members:["recognition-translation"], title:"识别与翻译", hint:"字幕偏好、模型与 API", icon:Cpu, keywords:"字幕 语音 转文字 SenseVoice FP16 GPU CUDA NVIDIA INT8 Whisper Ollama Qwen DeepSeek DS 密钥 本地 翻译 模型 下载 自动识别 自动翻译 词库 人名 术语 纠错 语言 轻声 呼气 拟声词 忠实 自然 柔和 标点 思考 温度 输出长度 token 参数 计费" },
  { id:"network-online", members:["network-online"], title:"网络与在线", hint:"镜像、代理与账户", icon:Globe, keywords:"api token 令牌 预设" },
  { id:"storage-library", members:["storage-library", "storage", "backup-sync"], title:"媒体库与数据", hint:"目录、清理与备份", icon:FolderOpen, keywords:"本地 下载 路径 实时 监控 扫描 存储 用量 空间 缓存 磁盘 恢复 同步 加密 WebDAV 导入 导出 保留" },
  { id:"updates", members:["updates", "cache-diagnostics"], title:"更新与诊断", hint:"版本、日志与排查", icon:RefreshCw, keywords:"软件 历史 检查 升级 说明 系统 错误 脱敏 报告" },
];

export const SettingsView: React.FC<SettingsViewProps> = ({ onError, initialSection = "privacy-window", onSectionChange, searchQuery, onSearchQueryChange }) => {
  const [localSection, setLocalSection] = useState(initialSection);
  const section = onSectionChange ? initialSection : localSection;
  const setSection = onSectionChange ?? setLocalSection;
  useEffect(() => { setLocalSection(initialSection); }, [initialSection]);
  const [localSearch, setLocalSearch] = useState("");
  const search = searchQuery ?? localSearch;
  const setSearch = onSearchQueryChange ?? setLocalSearch;
  const query = search.trim().toLowerCase();
  const visible = (id: string) => settingsSections.some(x => x.members.includes(id) && (query
    ? `${x.title} ${x.hint} ${x.keywords}`.toLowerCase().includes(query)
    : section === x.id));
  const recognitionVisible = visible("recognition-translation");
  const [recognitionVisited, setRecognitionVisited] = useState(recognitionVisible);
  useEffect(() => {
    if (recognitionVisible) setRecognitionVisited(true);
  }, [recognitionVisible]);
  const contentRef = useRef<HTMLDivElement>(null);
  usePageScroll(contentRef, `settings:${section}:${query}`);
  const [settings, setSettings] = useState<AppSettingsDto | null>(null);
  const [roots, setRoots] = useState<LibraryRootDto[]>([]);
  const [logs, setLogs] = useState<string[]>([]);

  const [downloadDir, setDownloadDir] = useState("");
  const [apiBaseUrl, setApiBaseUrl] = useState("https://api.asmr-200.com");
  const [proxyUrl, setProxyUrl] = useState("");
  const [closeBehavior, setCloseBehavior] = useState<"minimize_to_tray" | "quit">(
    "minimize_to_tray"
  );
  const [hardwareAcceleration, setHardwareAcceleration] = useState(true);
  const [audioExclusive, setAudioExclusive] = useState(false);
  const [smtcMode, setSmtcMode] = useState<"full" | "privacy" | "disabled">(
    "full"
  );
  const [realtimeWatchEnabled, setRealtimeWatchEnabled] = useState(true);
  const [webdavUrl, setWebdavUrl] = useState("");
  // NSFW switch defaults to OFF (false)
  const [nsfwEnabled, setNsfwEnabled] = useState(false);
  // Desktop Island preferences (T1308)
  const [islandFullscreenAvoidance, setIslandFullscreenAvoidance] = useState(true);
  const [islandEdgeSnap, setIslandEdgeSnap] = useState(true);
  const [islandAlwaysOnTop, setIslandAlwaysOnTop] = useState(true);
  const [islandScrollVolume, setIslandScrollVolume] = useState(true);
  const [islandToastEnabled, setIslandToastEnabled] = useState(true);
  const [islandToastDurationSec, setIslandToastDurationSec] = useState(3.0);
  const [islandPrivacyMode, setIslandPrivacyMode] = useState(false);
  const [authTokenInput, setAuthTokenInput] = useState("");
  const [savedNotice, setSavedNotice] = useState<{ id: number; message: string } | null>(null);
  const noticeId = useRef(0);
  const showNotice = (message: string) => setSavedNotice({ id: ++noticeId.current, message });
  const textAutoSaveTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const pendingTextRef = useRef<Record<string, string | null>>({});
  const saveQueueRef = useRef<Promise<unknown>>(Promise.resolve());

  const [relocatingRootId, setRelocatingRootId] = useState<string | null>(null);
  const [relocatePath, setRelocatePath] = useState("");

  const loadAll = useCallback(async () => {
    try {
      const [s, r, l] = await Promise.all([
        callCommand<AppSettingsDto>("get_app_settings"),
        callCommand<LibraryRootDto[]>("list_library_roots"),
        callCommand<string[]>("get_diagnostic_logs"),
      ]);
      setSettings(s);
      setDownloadDir(s.downloadDir);
      setApiBaseUrl(s.apiBaseUrl);
      setProxyUrl(s.proxyUrl ?? "");
      setCloseBehavior(s.closeBehavior);
      setHardwareAcceleration(s.hardwareAcceleration ?? true);
      setAudioExclusive(s.audioExclusive ?? false);
      setSmtcMode(s.smtcMode ?? "full");
      setRealtimeWatchEnabled(s.realtimeWatchEnabled ?? true);
      setWebdavUrl(s.webdavUrl ?? "");
      setNsfwEnabled(s.nsfwEnabled ?? false);
      setIslandFullscreenAvoidance(s.islandFullscreenAvoidance ?? true);
      setIslandEdgeSnap(s.islandEdgeSnap ?? true);
      setIslandAlwaysOnTop(s.islandAlwaysOnTop ?? true);
      setIslandScrollVolume(s.islandScrollVolume ?? true);
      setIslandToastEnabled(s.islandToastEnabled ?? true);
      setIslandToastDurationSec(s.islandToastDurationSec ?? 3.0);
      setIslandPrivacyMode(s.islandPrivacyMode ?? false);
      setRoots(r);
      setLogs(l);
    } catch (e) {
      onError(normalizeAppError(e));
    }
  }, [onError]);

  useEffect(() => {
    void loadAll();
  }, [loadAll]);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void subscribeSettingsUpdated(next => {
      if (disposed) return;
      setNsfwEnabled(next.nsfwEnabled ?? false);
      setIslandPrivacyMode(next.islandPrivacyMode ?? false);
      setIslandAlwaysOnTop(next.islandAlwaysOnTop ?? true);
    }).then(fn => { if (disposed) fn(); else unlisten = fn; });
    return () => { disposed = true; unlisten?.(); };
  }, []);

  const persistSettings = useCallback(
    async (
      overrides?: Partial<{
        downloadDir: string;
        apiBaseUrl: string;
        proxyUrl: string;
        closeBehavior: "minimize_to_tray" | "quit";
        hardwareAcceleration: boolean;
        audioExclusive: boolean;
        smtcMode: "full" | "privacy" | "disabled";
        realtimeWatchEnabled: boolean;
        webdavUrl: string;
        nsfwEnabled: boolean;
        islandFullscreenAvoidance: boolean;
        islandEdgeSnap: boolean;
        islandAlwaysOnTop: boolean;
        islandScrollVolume: boolean;
        islandToastEnabled: boolean;
        islandToastDurationSec: number;
        islandPrivacyMode: boolean;
        authToken: string | null;
      }>
    ) => {
      if (!settings) return;
      if (textAutoSaveTimerRef.current) {
        clearTimeout(textAutoSaveTimerRef.current);
        textAutoSaveTimerRef.current = null;
      }
      overrides = { ...pendingTextRef.current, ...overrides };
      pendingTextRef.current = {};
      const nextDownloadDir = (overrides?.downloadDir ?? downloadDir).trim();
      const nextApiBaseUrl = (overrides?.apiBaseUrl ?? apiBaseUrl).trim();
      const nextProxyUrl = (overrides?.proxyUrl ?? proxyUrl).trim() || null;
      const nextCloseBehavior = overrides?.closeBehavior ?? closeBehavior;
      const nextHardwareAcceleration =
        overrides?.hardwareAcceleration ?? hardwareAcceleration;
      const nextAudioExclusive = overrides?.audioExclusive ?? audioExclusive;
      const nextSmtcMode = overrides?.smtcMode ?? smtcMode;
      const nextRealtimeWatchEnabled =
        overrides?.realtimeWatchEnabled ?? realtimeWatchEnabled;
      const nextWebdavUrl = (overrides?.webdavUrl ?? webdavUrl).trim();
      const nextNsfwEnabled = overrides?.nsfwEnabled ?? nsfwEnabled;
      const nextIslandFullscreenAvoidance =
        overrides?.islandFullscreenAvoidance ?? islandFullscreenAvoidance;
      const nextIslandEdgeSnap = overrides?.islandEdgeSnap ?? islandEdgeSnap;
      const nextIslandAlwaysOnTop =
        overrides?.islandAlwaysOnTop ?? islandAlwaysOnTop;
      const nextIslandScrollVolume =
        overrides?.islandScrollVolume ?? islandScrollVolume;
      const nextIslandToastEnabled =
        overrides?.islandToastEnabled ?? islandToastEnabled;
      const nextIslandToastDurationSec =
        overrides?.islandToastDurationSec ?? islandToastDurationSec;
      const nextIslandPrivacyMode =
        overrides?.islandPrivacyMode ?? islandPrivacyMode;
      const nextAuthToken =
        overrides?.authToken !== undefined
          ? overrides.authToken
          : authTokenInput.trim() || null;

      try {
        const save = saveQueueRef.current.then(() => callCommand<AppSettingsDto>("update_app_settings", {
          downloadDir: nextDownloadDir,
          apiBaseUrl: nextApiBaseUrl,
          proxyUrl: nextProxyUrl,
          closeBehavior: nextCloseBehavior,
          hardwareAcceleration: nextHardwareAcceleration,
          ...(overrides?.audioExclusive !== undefined ? { audioExclusive: nextAudioExclusive } : {}),
          smtcMode: nextSmtcMode,
          realtimeWatchEnabled: nextRealtimeWatchEnabled,
          webdavUrl: nextWebdavUrl,
          nsfwEnabled: nextNsfwEnabled,
          islandFullscreenAvoidance: nextIslandFullscreenAvoidance,
          islandEdgeSnap: nextIslandEdgeSnap,
          islandAlwaysOnTop: nextIslandAlwaysOnTop,
          islandScrollVolume: nextIslandScrollVolume,
          islandToastEnabled: nextIslandToastEnabled,
          islandToastDurationSec: nextIslandToastDurationSec,
          islandPrivacyMode: nextIslandPrivacyMode,
          authToken: nextAuthToken,
        }));
        saveQueueRef.current = save.catch(() => {});
        const updated = await save;
        if (updated) {
          setSettings(updated);
          setAudioExclusive(updated.audioExclusive ?? false);
        }
        if (nextAuthToken && nextAuthToken !== "__CLEAR__") {
          setAuthTokenInput("");
        }
      } catch (err) {
        onError(normalizeAppError(err));
        if (overrides?.audioExclusive !== undefined) {
          void callCommand<AppSettingsDto>("get_app_settings").then((s) => setAudioExclusive(s.audioExclusive ?? false)).catch(() => {});
        }
      }
    },
    [
      downloadDir,
      apiBaseUrl,
      proxyUrl,
      closeBehavior,
      hardwareAcceleration,
      audioExclusive,
      smtcMode,
      realtimeWatchEnabled,
      webdavUrl,
      nsfwEnabled,
      islandFullscreenAvoidance,
      islandEdgeSnap,
      islandAlwaysOnTop,
      islandScrollVolume,
      islandToastEnabled,
      islandToastDurationSec,
      islandPrivacyMode,
      authTokenInput,
      onError,
      settings,
    ]
  );

  const persistSettingsRef = useRef(persistSettings);
  persistSettingsRef.current = persistSettings;

  useEffect(() => () => {
    if (textAutoSaveTimerRef.current) {
      clearTimeout(textAutoSaveTimerRef.current);
      // Flush the latest field values on navigation; never run an old render's closure.
      void persistSettingsRef.current();
    }
  }, []);

  const scheduleTextAutoSave = useCallback(
    (
      overrides: Partial<{
        downloadDir: string;
        apiBaseUrl: string;
        proxyUrl: string;
        webdavUrl: string;
        authToken: string | null;
      }>
    ) => {
      if (textAutoSaveTimerRef.current) {
        clearTimeout(textAutoSaveTimerRef.current);
      }
      pendingTextRef.current = { ...pendingTextRef.current, ...overrides };
      textAutoSaveTimerRef.current = setTimeout(() => {
        void persistSettingsRef.current();
      }, 450);
    },
    []
  );

  const handleSyncWatchedRootsNow = async () => {
    try {
      const reports = await callCommand<ScanReportDto[]>("sync_watched_roots_now");
      const added = reports.reduce((acc, r) => acc + r.worksAdded, 0);
      await loadAll();
      showNotice(
        reports.length === 0
          ? "所有监听目录指纹未变化，无需重扫"
          : `已完成变化目录增量同步（新增作品 ${added} 部）`
      );
    } catch (err) {
      onError(normalizeAppError(err));
    }
  };

  const handleClearAuthToken = () => persistSettings({ authToken: "__CLEAR__" });

  const handlePickDownloadDir = async () => {
    try {
      const picked = await open({
        directory: true,
        multiple: false,
        title: "选择离线下载保存根目录",
      });
      if (typeof picked === "string" && picked.trim()) {
        setDownloadDir(picked);
        void persistSettings({ downloadDir: picked });
      }
    } catch {
      // ignore cancel
    }
  };

  const handleRelocateRoot = async (rootId: string) => {
    if (!relocatePath.trim()) return;
    try {
      await callCommand<LibraryRootDto>("relocate_library_root", {
        rootId,
        newPath: relocatePath.trim(),
      });
      setRelocatingRootId(null);
      setRelocatePath("");
      await loadAll();
    } catch (err) {
      onError(normalizeAppError(err));
    }
  };

  const handleRescanRoot = async (root: LibraryRootDto) => {
    try {
      await callCommand<ScanReportDto>("scan_and_import", {
        rootPath: root.absolutePath,
        rootMode: root.rootMode,
        selectedCandidates: null,
      });
      await loadAll();
      showNotice(`已完成根目录增量复查: ${root.displayName}`);
    } catch (err) {
      onError(normalizeAppError(err));
    }
  };


  return (
    <div className="page-container settings-page-container">
      <header className="settings-page-header">
        <h2>设置</h2>
        <label className="settings-search"><Search size={18} aria-hidden="true" /><input type="search" value={search} onChange={e => { setSearch(e.target.value); }} placeholder="搜索设置…" aria-label="搜索设置" /></label>
      </header>
      <div className="settings-workspace">
        <nav className="settings-category-nav" aria-label="设置分类">
          {settingsSections.map(({id,title,hint,icon:Icon}) => <button key={id} type="button" aria-current={!query && section === id ? "page" : undefined} onClick={() => { setSection(id); setSearch(""); }}><Icon size={20} aria-hidden="true" /><span><strong>{title}</strong><small>{hint}</small></span></button>)}
        </nav>
        <div className="settings-content" ref={contentRef} role="region" aria-label="设置内容" tabIndex={0}>
      {savedNotice && <Toast key={savedNotice.id} message={savedNotice.message} onClose={() => setSavedNotice(null)} />}
      {query && !settingsSections.some(x => visible(x.id)) && <p className="settings-field-hint" role="status">没有找到相关设置，试试“字幕”“音频”或“网络”。</p>}
      <div hidden={!visible("updates")}><UpdateSettings onError={onError} /></div>
      {/* Load on first visit, then retain drafts and disclosure state across categories. */}
      <div hidden={!recognitionVisible}>{(recognitionVisible || recognitionVisited) && <RecognitionSettings />}</div>

      {/* 模块化设置表单 — 功能相近聚合为同一分组卡片，功能相异拆分为独立分组卡片 */}
      <form
        id="app-settings-form"
        onSubmit={(e) => {
          e.preventDefault();
          void persistSettings();
        }}
        className="settings-modular-stack"
        hidden={!["privacy-window","desktop-island","audio-engine","network-online","storage-library","desktop-lyrics"].some(visible)}
      >
        {/* 分组 1：内容安全、隐私防护与窗口行为 */}
        <section
          className="settings-card settings-group-card"
          data-settings-group="privacy-window" hidden={!visible("privacy-window")}
          aria-label="内容安全、隐私防护与窗口行为"
        >
          <div className="settings-group-header">
            <div className="settings-group-title-wrap">
              <span className="settings-group-icon">
                <ShieldAlert size={16} />
              </span>
              <div className="setting-label-help"><h3>内容安全、隐私防护与窗口行为</h3><HelpHint label={"内容安全、隐私防护与窗口行为"} text={<>
                  集中管理 NSFW 敏感内容默认过滤、Windows 锁屏/媒体浮窗防社死脱敏及窗口关闭驻留策略
                </>} /></div>
            </div>
            <span className="badge badge-tech">隐私与内容边界</span>
          </div>

          <div className="settings-group-body">
            <InterfaceScaleSettings />
            {/* NSFW 敏感内容显示开关（默认关闭） */}
            <div className="settings-row">
              <span className="setting-label-help"><label>成人向内容</label><HelpHint label={"NSFW 敏感/成人向内容显示开关（默认关闭）"} text={<>
                  默认关闭以自动过滤在线浏览、搜索与列表中的 R18 / NSFW 敏感作品；开启后展示全部内容
                </>} /></span>
              <div>
                <button
                  type="button"
                  className={`dsp-scope-toggle-btn ${
                    nsfwEnabled ? "is-lit" : "is-dim"
                  }`}
                  aria-label="切换 NSFW 敏感内容显示"
                  aria-pressed={nsfwEnabled}
                  onClick={() => {
                    const nextVal = !nsfwEnabled;
                    setNsfwEnabled(nextVal);
                    void persistSettings({ nsfwEnabled: nextVal });
                  }}
                >
                  {nsfwEnabled ? <Eye size={14} /> : <EyeOff size={14} />}
                  <span>
                    {nsfwEnabled
                      ? "显示成人向内容"
                      : "隐藏成人向内容"}
                  </span>
                </button>
              </div>
            </div>

            <div className="settings-row">
              <span className="setting-label-help"><label>
                  系统媒体信息
                </label><HelpHint label={"Windows SMTC 系统媒体浮层与隐私脱敏模式（键盘多媒体键控制）"} text={<>
                  控制 Windows 11 快捷媒体卡片、桌面音量浮窗与锁屏界面是否暴露真实封面与作品名
                </>} /></span>
              <div className="track-actions">
                <button
                  type="button"
                  className={`dsp-scope-toggle-btn ${
                    smtcMode === "full" ? "is-lit" : "is-dim"
                  }`}
                  aria-pressed={smtcMode === "full"}
                  onClick={() => {
                    setSmtcMode("full");
                    void callCommand("set_smtc_mode", { mode: "full" }).catch(
                      () => {}
                    );
                    void persistSettings({ smtcMode: "full" });
                  }}
                >
                  <span>完整展示</span>
                </button>
                <button
                  type="button"
                  className={`dsp-scope-toggle-btn ${
                    smtcMode === "privacy" ? "is-lit" : "is-dim"
                  }`}
                  aria-pressed={smtcMode === "privacy"}
                  onClick={() => {
                    setSmtcMode("privacy");
                    void callCommand("set_smtc_mode", {
                      mode: "privacy",
                    }).catch(() => {});
                    void persistSettings({ smtcMode: "privacy" });
                  }}
                >
                  <span>隐私模式</span>
                </button>
                <button
                  type="button"
                  className={`dsp-scope-toggle-btn ${
                    smtcMode === "disabled" ? "is-lit" : "is-dim"
                  }`}
                  aria-pressed={smtcMode === "disabled"}
                  onClick={() => {
                    setSmtcMode("disabled");
                    void callCommand("set_smtc_mode", {
                      mode: "disabled",
                    }).catch(() => {});
                    void persistSettings({ smtcMode: "disabled" });
                  }}
                >
                  <span>关闭</span>
                </button>
              </div>
            </div>

            <div className="settings-row">
              <span className="setting-label-help"><label htmlFor="setting-close">点击窗口关闭按钮时</label><HelpHint label={"点击窗口关闭按钮时"} text={<>
                  选择点击右上角关闭按钮时最小化到托盘继续助眠播放，或彻底退出进程
                </>} /></span>
              <Select
                id="setting-close"
                className="settings-control-wide"
                value={closeBehavior}
                onValueChange={(value) => {
                  const nextVal = value as "minimize_to_tray" | "quit";
                  setCloseBehavior(nextVal);
                  void persistSettings({ closeBehavior: nextVal });
                }}
                options={[
                  { value: "minimize_to_tray", label: "隐藏到系统托盘并保持后台播放" },
                  { value: "quit", label: "保存播放状态、回收 mpv 进程并退出应用" },
                ]}
              />
            </div>
          </div>
        </section>

        {/* 分组：桌面灵动岛偏好设置 (T1308) */}
        <section
          className="settings-card settings-group-card"
          data-settings-group="desktop-island" hidden={!visible("desktop-island")}
          aria-label="桌面灵动岛偏好设置"
        >
          <div className="settings-group-header">
            <div className="settings-group-title-wrap">
              <span className="settings-group-icon">
                <AppWindow size={16} />
              </span>
              <div className="setting-label-help"><h3>桌面灵动岛偏好设置</h3><HelpHint label={"桌面灵动岛偏好设置"} text={<>
                  调整显示屏幕、大小与播放交互
                </>} /></div>
            </div>
          </div>

          <div className="settings-group-body">
            {/* 1. 全屏独占避让开关 */}
            <SettingsDisclosure title="屏幕与外观" description="选择显示屏幕，调整灵动岛大小、字号与字重"><IslandDisplaySettings /></SettingsDisclosure>
            <div className="settings-row">
              <span className="setting-label-help"><label>全屏时自动隐藏</label><HelpHint label={"全屏独占避让（全屏视频与游戏时自动淡出隐藏）"} text={<>
                  当检测到其他软件（全屏游戏、全屏看剧）进入全屏独占时，灵动岛自动平滑淡出隐藏，退出全屏后恢复显示
                </>} /></span>
              <div>
                <button
                  type="button"
                  className={`dsp-scope-toggle-btn ${
                    islandFullscreenAvoidance ? "is-lit" : "is-dim"
                  }`}
                  aria-pressed={islandFullscreenAvoidance}
                  aria-label="切换灵动岛全屏避让"
                  onClick={() => {
                    const nextVal = !islandFullscreenAvoidance;
                    setIslandFullscreenAvoidance(nextVal);
                    void persistSettings({ islandFullscreenAvoidance: nextVal });
                  }}
                >
                  <span>
                    {islandFullscreenAvoidance
                      ? "已开启自动避让"
                      : "始终显示"}
                  </span>
                </button>
              </div>
            </div>

            {/* 2. 边缘弹性磁吸开关 */}
            <div className="settings-row">
              <span className="setting-label-help"><label>贴近屏幕顶部时吸附</label><HelpHint label={"边缘弹性磁吸（靠近屏幕顶部 30px 自动贴边）"} text={<>
                  拖拽常态胶囊松开时，若光标距离屏幕或副屏顶部小于 30px，自动弹性吸附贴边并持久化记忆坐标
                </>} /></span>
              <div>
                <button
                  type="button"
                  className={`dsp-scope-toggle-btn ${
                    islandEdgeSnap ? "is-lit" : "is-dim"
                  }`}
                  aria-pressed={islandEdgeSnap}
                  aria-label="切换灵动岛边缘弹性磁吸"
                  onClick={() => {
                    const nextVal = !islandEdgeSnap;
                    setIslandEdgeSnap(nextVal);
                    void persistSettings({ islandEdgeSnap: nextVal });
                  }}
                >
                  <Magnet size={14} />
                  <span>
                    {islandEdgeSnap
                      ? "已开启边缘吸附"
                      : "自由停靠"}
                  </span>
                </button>
              </div>
            </div>

            {/* 3. 灵动岛默认置顶开关 */}
            <div className="settings-row">
              <span className="setting-label-help"><label>窗口置顶</label><HelpHint label={"灵动岛窗口置顶（Always on Top）"} text={<>
                  保持桌面灵动岛始终浮于常规桌面窗口之上，方便随时查看同步台词与快速播控
                </>} /></span>
              <div>
                <button
                  type="button"
                  className={`dsp-scope-toggle-btn ${
                    islandAlwaysOnTop ? "is-lit" : "is-dim"
                  }`}
                  aria-pressed={islandAlwaysOnTop}
                  aria-label="切换灵动岛窗口置顶"
                  onClick={() => {
                    const nextVal = !islandAlwaysOnTop;
                    setIslandAlwaysOnTop(nextVal);
                    void callCommand("island_set_always_on_top", { alwaysOnTop: nextVal }).catch(() => {});
                    void persistSettings({ islandAlwaysOnTop: nextVal });
                  }}
                >
                  <Pin size={14} />
                  <span>
                    {islandAlwaysOnTop
                      ? "已开启置顶"
                      : "已关闭置顶"}
                  </span>
                </button>
              </div>
            </div>

            {/* 4. 紧凑态滚轮调音开关 */}
            <div className="settings-row">
              <span className="setting-label-help"><label>滚轮调节音量</label><HelpHint label={"常态胶囊滚轮调音（无需展开控制舱）"} text={<>
                  鼠标悬停在常态紧凑胶囊上直接滚动鼠标滚轮即可按步进快速微调播放音量并弹出数值反馈
                </>} /></span>
              <div>
                <button
                  type="button"
                  className={`dsp-scope-toggle-btn ${
                    islandScrollVolume ? "is-lit" : "is-dim"
                  }`}
                  aria-pressed={islandScrollVolume}
                  aria-label="切换灵动岛滚轮调音"
                  onClick={() => {
                    const nextVal = !islandScrollVolume;
                    setIslandScrollVolume(nextVal);
                    void persistSettings({ islandScrollVolume: nextVal });
                  }}
                >
                  <Volume2 size={14} />
                  <span>
                    {islandScrollVolume
                      ? "已开启滚轮调音"
                      : "已关闭滚轮调音"}
                  </span>
                </button>
              </div>
            </div>

            <IslandToastSettings enabled={islandToastEnabled} duration={islandToastDurationSec}
              onToggle={enabled => { setIslandToastEnabled(enabled); void persistSettings({ islandToastEnabled: enabled }); }}
              onDurationCommit={seconds => { setIslandToastDurationSec(seconds); void persistSettings({ islandToastDurationSec: seconds }); }} />

            {/* 6. 灵动岛隐私脱敏模式开关 */}
            <div className="settings-row">
              <span className="setting-label-help"><label>隐藏封面与标题</label><HelpHint label={"灵动岛防社死隐私脱敏模式"} text={<>
                  隐藏敏感同人插画封面，将曲目名称统一显示为中性占位文本 "ASMR Audio Track"
                </>} /></span>
              <div>
                <button
                  type="button"
                  className={`dsp-scope-toggle-btn ${
                    islandPrivacyMode ? "is-lit" : "is-dim"
                  }`}
                  aria-pressed={islandPrivacyMode}
                  aria-label="切换灵动岛隐私脱敏模式"
                  onClick={() => {
                    const nextVal = !islandPrivacyMode;
                    setIslandPrivacyMode(nextVal);
                    void callCommand("island_set_privacy_mode", { enabled: nextVal }).catch(() => {});
                    void persistSettings({ islandPrivacyMode: nextVal });
                  }}
                >
                  {islandPrivacyMode ? <EyeOff size={14} /> : <Eye size={14} />}
                  <span>
                    {islandPrivacyMode
                      ? "已开启隐私模式"
                      : "显示封面与标题"}
                  </span>
                </button>
              </div>
            </div>
          </div>
        </section>

        {/* 分组 2：音频引擎、硬件直通与解码加速 */}
        <div hidden={!visible("desktop-lyrics")}><DesktopLyricsSettings /></div>
        <section
          className="settings-card settings-group-card"
          data-settings-group="audio-engine" hidden={!visible("audio-engine")}
          aria-label="音频引擎、硬件直通与解码加速"
        >
          <div className="settings-group-header">
            <div className="settings-group-title-wrap">
              <span className="settings-group-icon">
                <Cpu size={16} />
              </span>
              <div className="setting-label-help"><h3>音频引擎、硬件直通与解码加速</h3><HelpHint label={"音频引擎、硬件直通与解码加速"} text={<>
                  管理 mpv 音频宿主状态、WASAPI 声卡独占原生采样率直通以及 WebView2 / 解码硬件加速
                </>} /></div>
            </div>
            <span className="badge badge-tech">高保真声学核心</span>
          </div>

          <div className="settings-group-body">
            <div className="settings-row">
              <span className="setting-label-help"><label>libmpv 进程内引擎状态 (mpv-2.dll)</label><HelpHint label={"libmpv 进程内引擎状态 (mpv-2.dll)"} text={<>
                  通过主进程内 FFI 原生调用 libmpv C-API 驱动无损解码、200% 音量上限与 8 级 lavfi 音效链路（无独立子进程）
                </>} /></span>
              <div>
                {settings?.mpvAvailable ? (
                  <span className="badge badge-downloaded">
                    <CheckCircle2 size={12} />
                    <span>引擎就绪 ({settings.mpvPath})</span>
                  </span>
                ) : (
                  <span className="badge badge-warn">
                    <AlertTriangle size={12} />
                    <span>未检测到 libmpv 动态库 (mpv-2.dll)</span>
                  </span>
                )}
              </div>
            </div>

            <div className="settings-row">
              <span className="setting-label-help"><label>WASAPI 独占输出</label><HelpHint label={"WASAPI 独占直通模式（绕过系统混音器重采样）"} text={<>
                  直接接管声卡/DAC 按原采样率（如 96kHz/24bit）输出；也可在底栏「控制与音效」中热切换
                </>} /></span>
              <div>
                <button
                  type="button"
                  className={`dsp-scope-toggle-btn ${
                    audioExclusive ? "is-lit" : "is-dim"
                  }`}
                  aria-pressed={audioExclusive}
                  onClick={() => {
                    const nextVal = !audioExclusive;
                    setAudioExclusive(nextVal);
                    void persistSettings({ audioExclusive: nextVal });
                  }}
                >
                  <span>
                    {audioExclusive
                      ? "独占输出已开启"
                      : "使用共享输出"}
                  </span>
                </button>
              </div>
            </div>

            <div className="settings-row">
              <span className="setting-label-help"><label>硬件加速</label><HelpHint label={"启用硬件加速（GPU 界面渲染与媒体解码加速）"} text={<>
                  开启 GPU 光栅化与 AVX2 多线程硬解加速，显著降低高分屏与长音频播放时的 CPU 占用
                </>} /></span>
              <div>
                <button
                  type="button"
                  className={`dsp-scope-toggle-btn ${
                    hardwareAcceleration ? "is-lit" : "is-dim"
                  }`}
                  aria-pressed={hardwareAcceleration}
                  onClick={() => {
                    const nextVal = !hardwareAcceleration;
                    setHardwareAcceleration(nextVal);
                    void persistSettings({ hardwareAcceleration: nextVal });
                  }}
                >
                  <Cpu size={14} />
                  <span>
                    {hardwareAcceleration
                      ? "已开启硬件加速"
                      : "使用软件模式"}
                  </span>
                </button>
              </div>
            </div>
          </div>
        </section>

        <section className="settings-card" hidden={!visible("audio-engine")} aria-label="引擎定期维护">
          <h3>定期重启引擎</h3>
          <EngineMaintenanceSettings />
        </section>

        {/* 分组 3：在线源站、网络代理与远程挂载 */}
        <section
          className="settings-card settings-group-card"
          data-settings-group="network-online" hidden={!visible("network-online")}
          aria-label="在线源站、网络代理与远程挂载"
        >
          <div className="settings-group-header">
            <div className="settings-group-title-wrap">
              <span className="settings-group-icon">
                <Globe size={16} />
              </span>
              <div className="setting-label-help"><h3>在线源站、网络代理与远程挂载</h3><HelpHint label={"在线源站、网络代理与远程挂载"} text={<>
                  独立配置 asmr.one 镜像节点、HTTP/SOCKS5 网络代理、站点访问凭据及局域网 WebDAV/NAS 流式地址
                </>} /></div>
            </div>
            <span className="badge badge-tech">网络与在线连接</span>
          </div>

          <div className="settings-group-body">
            <div className="settings-row">
              <span className="setting-label-help"><label htmlFor="setting-api-base">asmr.one API 基址</label><HelpHint label={"asmr.one API 基址"} text={<>
                  用于在线浏览、元数据同步与云端字幕抓取的站点 API 域名，支持一键切换官方镜像
                </>} /></span>
              <div className="input-with-btn">
                <input
                  id="setting-api-base"
                  type="text"
                  className="input-text"
                  value={apiBaseUrl}
                  onChange={(e) => {
                    const nextVal = e.target.value;
                    setApiBaseUrl(nextVal);
                    scheduleTextAutoSave({ apiBaseUrl: nextVal });
                  }}
                  onBlur={() => void persistSettings({ apiBaseUrl })}
                />
                <Select
                  size="compact"
                  aria-label="镜像预设"
                  value=""
                  onValueChange={(value) => {
                    if (value) {
                      const nextVal = value;
                      setApiBaseUrl(nextVal);
                      void persistSettings({ apiBaseUrl: nextVal });
                    }
                  }}
                  options={[
                    { value: "", label: "镜像预设..." },
                    ...["api.asmr-200.com", "api.asmr.one", "api.asmr-100.com", "api.asmr-300.com"].map(host => ({ value: `https://${host}`, label: host })),
                  ]}
                />
              </div>
            </div>

            <div className="settings-row">
              <span className="setting-label-help"><label htmlFor="setting-proxy">
                  网络代理
                </label><HelpHint label={"HTTP / SOCKS5 代理 (同时作用于 API 与 mpv)"} text={<>
                  留空则直接连接；配置后将同步作用于在线接口请求、离线下载与 mpv 流式播放
                </>} /></span>
              <input
                id="setting-proxy"
                type="text"
                className="input-text settings-control-wide"
                placeholder="留空直连，或填写如 http://127.0.0.1:7890"
                value={proxyUrl}
                onChange={(e) => {
                  const nextVal = e.target.value;
                  setProxyUrl(nextVal);
                  scheduleTextAutoSave({ proxyUrl: nextVal });
                }}
                onBlur={() => void persistSettings({ proxyUrl })}
              />
            </div>

            <SettingsDisclosure title="访问凭据与远程目录" description="站点令牌、WebDAV 与 NAS 地址">
            <div className="settings-row">
              <span className="setting-label-help"><label htmlFor="setting-token">
                  站点访问令牌
                </label><HelpHint label={"站点 Bearer 凭据 (存于 Windows 系统凭据管理器)"} text={<>
                  令牌通过系统级 Keyring 加密存储，绝不明文写入数据库或日志
                </>} /></span>
              <div className="input-with-btn">
                <input
                  id="setting-token"
                  type="password"
                  className="input-text"
                  placeholder={
                    settings?.hasAuthToken
                      ? "已保存系统凭据（输入新值可覆盖）"
                      : "可选：输入站点访问令牌"
                  }
                  value={authTokenInput}
                  onChange={(e) => {
                    const nextVal = e.target.value;
                    setAuthTokenInput(nextVal);
                    if (nextVal.trim()) {
                      scheduleTextAutoSave({ authToken: nextVal.trim() });
                    }
                  }}
                  onBlur={() => {
                    if (authTokenInput.trim()) {
                      void persistSettings({ authToken: authTokenInput.trim() });
                    }
                  }}
                />
                {settings?.hasAuthToken && (
                  <button
                    type="button"
                    className="btn btn-ghost btn-sm"
                    onClick={() => void handleClearAuthToken()}
                  >
                    清除已存凭据
                  </button>
                )}
              </div>
            </div>

            <div className="settings-row">
              <span className="setting-label-help"><label htmlFor="setting-webdav">
                  WebDAV / NAS 地址
                </label><HelpHint label={"局域网 WebDAV / NAS 挂载流式地址"} text={<>
                  可选挂载家庭 NAS 或 WebDAV 共享目录，支持跨局域网直接流式收听大容量无损作品
                </>} /></span>
              <input
                id="setting-webdav"
                type="text"
                className="input-text settings-control-wide"
                placeholder="可选：如 http://192.168.1.10:5005/asmr 或 \\NAS\ASMR"
                value={webdavUrl}
                onChange={(e) => {
                  const nextVal = e.target.value;
                  setWebdavUrl(nextVal);
                  scheduleTextAutoSave({ webdavUrl: nextVal });
                }}
                onBlur={() => void persistSettings({ webdavUrl })}
              />
            </div>
            </SettingsDisclosure>
          </div>
        </section>

        {/* 分组 4：本地存储、目录实时监听与音频库根目录 */}
        <section
          className="settings-card settings-group-card"
          data-settings-group="storage-library" hidden={!visible("storage-library")}
          aria-label="本地存储、目录实时监听与音频库根目录"
        >
          <div className="settings-group-header">
            <div className="settings-group-title-wrap">
              <span className="settings-group-icon">
                <HardDrive size={16} />
              </span>
              <div className="setting-label-help"><h3>本地存储、目录实时监听与音频库根目录</h3><HelpHint label={"本地存储、目录实时监听与音频库根目录"} text={<>
                  统一管理离线下载存放目录、后台目录指纹增量监听以及外接硬盘盘符变更后的无损重定位
                </>} /></div>
            </div>
            <span className="badge badge-tech">本地磁盘与目录树</span>
          </div>

          <div className="settings-group-body">
            <div className="settings-row">
              <span className="setting-label-help"><label htmlFor="setting-dl-dir">离线下载根目录</label><HelpHint label={"离线下载根目录"} text={<>
                  在线作品下载完成后的本地存放根目录（下载中自动隔离为 .part 临时文件）
                </>} /></span>
              <div className="input-with-btn">
                <input
                  id="setting-dl-dir"
                  type="text"
                  className="input-text"
                  value={downloadDir}
                  onChange={(e) => {
                    const nextVal = e.target.value;
                    setDownloadDir(nextVal);
                    scheduleTextAutoSave({ downloadDir: nextVal });
                  }}
                  onBlur={() => void persistSettings({ downloadDir })}
                />
                <button
                  type="button"
                  className="btn btn-secondary btn-sm"
                  onClick={() => void handlePickDownloadDir()}
                >
                  <FolderOpen size={14} />
                  <span>浏览...</span>
                </button>
              </div>
            </div>

            <div className="settings-row">
              <span className="setting-label-help"><label>自动同步音频库</label><HelpHint label={"目录实时文件监听与无感增量同步"} text={<>
                  后台自动比对已导入根目录指纹，仅在新增或变更作品时触发秒级无感增量入库
                </>} /></span>
              <div className="track-actions">
                <button
                  type="button"
                  className={`dsp-scope-toggle-btn ${
                    realtimeWatchEnabled ? "is-lit" : "is-dim"
                  }`}
                  aria-pressed={realtimeWatchEnabled}
                  onClick={() => {
                    const nextVal = !realtimeWatchEnabled;
                    setRealtimeWatchEnabled(nextVal);
                    void persistSettings({ realtimeWatchEnabled: nextVal });
                  }}
                >
                  <span>
                    {realtimeWatchEnabled
                      ? "已开启自动同步"
                      : "已暂停自动同步"}
                  </span>
                </button>
                <button
                  type="button"
                  className="btn btn-secondary btn-sm"
                  onClick={() => void handleSyncWatchedRootsNow()}
                >
                  <RefreshCw size={13} />
                  <span>立即同步</span>
                </button>
              </div>
            </div>

            <SettingsDisclosure title="音频库目录" description="查看已导入目录、增量扫描与路径重定位" summary={`${roots.length} 个目录`}>
              <div className="settings-table-scroll" role="region" aria-label="音频库目录列表" tabIndex={0}>
              {roots.length === 0 ? (
                <div className="empty-box">尚未导入任何本地根目录</div>
              ) : (
                <table className="preview-table">
                  <thead>
                    <tr>
                      <th>名称</th>
                      <th>模式</th>
                      <th>当前路径</th>
                      <th>状态</th>
                      <th>作品数</th>
                      <th>操作</th>
                    </tr>
                  </thead>
                  <tbody>
                    {roots.map((r) => (
                      <React.Fragment key={r.id}>
                        <tr>
                          <td>{r.displayName}</td>
                          <td>
                            {r.rootMode === "single_work"
                              ? "单作品目录"
                              : "作品库根目录"}
                          </td>
                          <td>
                            <code className="code-inline">{r.absolutePath}</code>
                          </td>
                          <td>
                            <span
                              className={`badge ${
                                r.availability === "available"
                                  ? "badge-local"
                                  : "badge-warn"
                              }`}
                            >
                              {r.availability === "available"
                                ? "在线可用"
                                : "磁盘离线"}
                            </span>
                          </td>
                          <td>{r.workCount}</td>
                          <td>
                            <div className="track-actions">
                              <button
                                type="button"
                                className="btn btn-secondary btn-sm"
                                onClick={() => void handleRescanRoot(r)}
                              >
                                <RefreshCw size={13} />
                                <span>增量扫描</span>
                              </button>
                              <button
                                type="button"
                                className="btn btn-ghost btn-sm"
                                onClick={() => {
                                  setRelocatingRootId(r.id);
                                  setRelocatePath(r.absolutePath);
                                }}
                              >
                                重新定位路径
                              </button>
                              <RemoveLibraryRootButton root={r} onError={onError} onRemoved={removed => {
                                setRoots(current => current.filter(root => root.id !== removed.id));
                                if (relocatingRootId === removed.id) { setRelocatingRootId(null); setRelocatePath(""); }
                                showNotice(`已移除目录引用：${removed.displayName}`);
                              }} />
                            </div>
                          </td>
                        </tr>
                        {relocatingRootId === r.id && (
                          <tr>
                            <td colSpan={6}>
                              <div className="input-with-btn">
                                <input
                                  type="text"
                                  className="input-text"
                                  placeholder="输入移动硬盘的新盘符或新目录绝对路径..."
                                  value={relocatePath}
                                  onChange={(e) =>
                                    setRelocatePath(e.target.value)
                                  }
                                />
                                <button
                                  type="button"
                                  className="btn btn-secondary btn-sm"
                                  onClick={() => void handleRelocateRoot(r.id)}
                                >
                                  确认重定位并复查文件
                                </button>
                                <button
                                  type="button"
                                  className="btn btn-ghost btn-sm"
                                  onClick={() => setRelocatingRootId(null)}
                                >
                                  取消
                                </button>
                              </div>
                            </td>
                          </tr>
                        )}
                      </React.Fragment>
                    ))}
                  </tbody>
                </table>
              )}
              </div>
            </SettingsDisclosure>
          </div>
        </section>
      </form>

      <div hidden={!visible("storage")}><StorageSettings /></div>
      <div hidden={!visible("backup-sync")}><BackupSettings onRestored={loadAll} /></div>
      <div hidden={!visible("cache-diagnostics")}><DiagnosticSettings /></div>

      {/* 分组 6：脱敏诊断日志 */}
      <section
        className="settings-card settings-group-card"
        data-settings-group="cache-diagnostics" hidden={!visible("cache-diagnostics")}
        aria-label="脱敏诊断日志"
      >
        <div className="settings-group-header">
          <div className="settings-group-title-wrap">
            <span className="settings-group-icon">
              <Terminal size={16} />
            </span>
            <div className="setting-label-help"><h3>脱敏诊断日志</h3><HelpHint label={"脱敏诊断日志"} text={<>
                查看已自动脱敏令牌的宿主运行日志
              </>} /></div>
          </div>
          <div className="track-actions">
            <button
              type="button"
              className="btn btn-ghost btn-sm"
              onClick={() => void loadAll()}
            >
              <RefreshCw size={14} />
              <span>刷新日志</span>
            </button>
          </div>
        </div>
        <div className="settings-group-body"><SettingsDisclosure title="最近运行日志" description="查看已脱敏的日志内容" summary={`${logs.length} 条记录`}>
        <div className="log-box">
          <div className="log-header">
            <Terminal size={14} />
            <span>最近运行日志（已自动脱敏令牌与敏感参数）</span>
          </div>
          <pre className="log-pre">
            {logs.length === 0 ? "暂无日志记录" : logs.join("\n")}
          </pre>
        </div>
        </SettingsDisclosure></div>
      </section>

      <div hidden={!visible("shortcuts")}><ShortcutSettings smtcMode={settings?.smtcMode} /></div>
        </div>
      </div>
    </div>
  );
};
