import { Select } from "../../components/Select";
import React, { useEffect, useState, useCallback, useRef } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import {
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

interface SettingsViewProps {
  onError: (err: AppError) => void;
}

export const SettingsView: React.FC<SettingsViewProps> = ({ onError }) => {
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
  const [savedNotice, setSavedNotice] = useState("");
  const noticeTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
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
        setSavedNotice("设置已保存并立即生效");
        if (noticeTimerRef.current) {
          clearTimeout(noticeTimerRef.current);
        }
        noticeTimerRef.current = setTimeout(() => setSavedNotice(""), 2000);
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
    if (noticeTimerRef.current) clearTimeout(noticeTimerRef.current);
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
      setSavedNotice(
        reports.length === 0
          ? "所有监听目录指纹未变化，无需重扫"
          : `已完成变化目录增量同步（新增作品 ${added} 部）`
      );
      setTimeout(() => setSavedNotice(""), 3000);
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
      setSavedNotice(`已完成根目录增量复查: ${root.displayName}`);
      setTimeout(() => setSavedNotice(""), 2500);
    } catch (err) {
      onError(normalizeAppError(err));
    }
  };


  return (
    <div className="page-container settings-page-container">
      <header className="settings-page-header">
        <div>
          <h2>设置与系统诊断</h2>
          <p className="settings-page-subtitle">
            管理内容隐私、音频引擎、在线网络、本地存储与快捷键。常规配置自动保存，快捷键确认保存后生效。
          </p>
        </div>
      </header>

      {savedNotice && <div className="info-banner">{savedNotice}</div>}
      <UpdateSettings onError={onError} />

      {/* 模块化设置表单 — 功能相近聚合为同一分组卡片，功能相异拆分为独立分组卡片 */}
      <form
        id="app-settings-form"
        onSubmit={(e) => {
          e.preventDefault();
          void persistSettings();
        }}
        className="settings-modular-stack"
      >
        {/* 分组 1：内容安全、隐私防护与窗口行为 */}
        <section
          className="settings-card settings-group-card"
          data-settings-group="privacy-window"
          aria-label="内容安全、隐私防护与窗口行为"
        >
          <div className="settings-group-header">
            <div className="settings-group-title-wrap">
              <span className="settings-group-icon">
                <ShieldAlert size={16} />
              </span>
              <div>
                <h3>内容安全、隐私防护与窗口行为</h3>
                <p className="settings-group-desc">
                  集中管理 NSFW 敏感内容默认过滤、Windows 锁屏/媒体浮窗防社死脱敏及窗口关闭驻留策略
                </p>
              </div>
            </div>
            <span className="badge badge-tech">隐私与内容边界</span>
          </div>

          <div className="settings-group-body">
            {/* NSFW 敏感内容显示开关（默认关闭） */}
            <div className="settings-row">
              <div className="settings-label-block">
                <label>NSFW 敏感/成人向内容显示开关（默认关闭）</label>
                <span className="settings-field-hint">
                  默认关闭以自动过滤在线浏览、搜索与列表中的 R18 / NSFW 敏感作品；开启后展示全部内容
                </span>
              </div>
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
                    void callCommand<boolean>("set_nsfw_enabled", {
                      enabled: nextVal,
                    }).catch(() => {});
                    void persistSettings({ nsfwEnabled: nextVal });
                  }}
                >
                  {nsfwEnabled ? <Eye size={14} /> : <EyeOff size={14} />}
                  <span>
                    {nsfwEnabled
                      ? "已开启 NSFW 敏感内容显示 (允许展示 R18 作品)"
                      : "已隐藏 NSFW 敏感内容 (默认关闭，自动过滤 R18 作品)"}
                  </span>
                </button>
              </div>
            </div>

            <div className="settings-row">
              <div className="settings-label-block">
                <label>
                  Windows SMTC 系统媒体浮层与隐私脱敏模式（键盘多媒体键控制）
                </label>
                <span className="settings-field-hint">
                  控制 Windows 11 快捷媒体卡片、桌面音量浮窗与锁屏界面是否暴露真实封面与作品名
                </span>
              </div>
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
                  <span>完整展示 (真实标题与封面)</span>
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
                  <span>隐私脱敏 (展示 ASMR Audio Track 并隐藏封面)</span>
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
                  <span>完全关闭 SMTC</span>
                </button>
              </div>
            </div>

            <div className="settings-row">
              <div className="settings-label-block">
                <label htmlFor="setting-close">点击窗口关闭按钮时</label>
                <span className="settings-field-hint">
                  选择点击右上角关闭按钮时最小化到托盘继续助眠播放，或彻底退出进程
                </span>
              </div>
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
          data-settings-group="desktop-island"
          aria-label="桌面灵动岛偏好设置"
        >
          <div className="settings-group-header">
            <div className="settings-group-title-wrap">
              <span className="settings-group-icon">
                <AppWindow size={16} />
              </span>
              <div>
                <h3>桌面灵动岛偏好设置</h3>
                <p className="settings-group-desc">
                  调整显示屏幕、大小与播放交互
                </p>
              </div>
            </div>
          </div>

          <div className="settings-group-body">
            {/* 1. 全屏独占避让开关 */}
            <IslandDisplaySettings />
            <div className="settings-row">
              <div className="settings-label-block">
                <label>全屏独占避让（全屏视频与游戏时自动淡出隐藏）</label>
                <span className="settings-field-hint">
                  当检测到其他软件（全屏游戏、全屏看剧）进入全屏独占时，灵动岛自动平滑淡出隐藏，退出全屏后恢复显示
                </span>
              </div>
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
                      ? "已启用全屏自动避让淡出"
                      : "已关闭全屏避让（始终保持显示）"}
                  </span>
                </button>
              </div>
            </div>

            {/* 2. 边缘弹性磁吸开关 */}
            <div className="settings-row">
              <div className="settings-label-block">
                <label>边缘弹性磁吸（靠近屏幕顶部 30px 自动贴边）</label>
                <span className="settings-field-hint">
                  拖拽常态胶囊松开时，若光标距离屏幕或副屏顶部小于 30px，自动弹性吸附贴边并持久化记忆坐标
                </span>
              </div>
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
                      ? "已开启 30px 屏幕顶部弹性磁吸"
                      : "已关闭磁吸（支持自由像素停靠）"}
                  </span>
                </button>
              </div>
            </div>

            {/* 3. 灵动岛默认置顶开关 */}
            <div className="settings-row">
              <div className="settings-label-block">
                <label>灵动岛窗口置顶（Always on Top）</label>
                <span className="settings-field-hint">
                  保持桌面灵动岛始终浮于常规桌面窗口之上，方便随时查看同步台词与快速播控
                </span>
              </div>
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
                      ? "已开启桌面置顶 (Always on Top)"
                      : "已关闭置顶（允许被其他窗口覆盖）"}
                  </span>
                </button>
              </div>
            </div>

            {/* 4. 紧凑态滚轮调音开关 */}
            <div className="settings-row">
              <div className="settings-label-block">
                <label>常态胶囊滚轮调音（无需展开控制舱）</label>
                <span className="settings-field-hint">
                  鼠标悬停在常态紧凑胶囊上直接滚动鼠标滚轮即可按步进快速微调播放音量并弹出数值反馈
                </span>
              </div>
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
                      ? "已开启常态胶囊滚轮调音"
                      : "已关闭常态滚轮调音"}
                  </span>
                </button>
              </div>
            </div>

            <IslandToastSettings enabled={islandToastEnabled} duration={islandToastDurationSec}
              onToggle={enabled => { setIslandToastEnabled(enabled); void persistSettings({ islandToastEnabled: enabled }); }}
              onDurationCommit={seconds => { setIslandToastDurationSec(seconds); void persistSettings({ islandToastDurationSec: seconds }); }} />

            {/* 6. 灵动岛隐私脱敏模式开关 */}
            <div className="settings-row">
              <div className="settings-label-block">
                <label>灵动岛防社死隐私脱敏模式</label>
                <span className="settings-field-hint">
                  隐藏敏感同人插画封面，将曲目名称统一显示为中性占位文本 "ASMR Audio Track"
                </span>
              </div>
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
                      ? "已开启脱敏模式 (隐藏封面与敏感标题)"
                      : "已关闭脱敏模式 (正常展示封面与标题)"}
                  </span>
                </button>
              </div>
            </div>
          </div>
        </section>

        {/* 分组 2：音频引擎、硬件直通与解码加速 */}
        <section
          className="settings-card settings-group-card"
          data-settings-group="audio-engine"
          aria-label="音频引擎、硬件直通与解码加速"
        >
          <div className="settings-group-header">
            <div className="settings-group-title-wrap">
              <span className="settings-group-icon">
                <Cpu size={16} />
              </span>
              <div>
                <h3>音频引擎、硬件直通与解码加速</h3>
                <p className="settings-group-desc">
                  管理 mpv 音频宿主状态、WASAPI 声卡独占原生采样率直通以及 WebView2 / 解码硬件加速
                </p>
              </div>
            </div>
            <span className="badge badge-tech">高保真声学核心</span>
          </div>

          <div className="settings-group-body">
            <div className="settings-row">
              <div className="settings-label-block">
                <label>libmpv 进程内引擎状态 (mpv-2.dll)</label>
                <span className="settings-field-hint">
                  通过主进程内 FFI 原生调用 libmpv C-API 驱动无损解码、200% 音量上限与 8 级 lavfi 音效链路（无独立子进程）
                </span>
              </div>
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
              <div className="settings-label-block">
                <label>WASAPI 独占直通模式（绕过系统混音器重采样）</label>
                <span className="settings-field-hint">
                  直接接管声卡/DAC 按原采样率（如 96kHz/24bit）输出；也可在底栏「控制与音效」中热切换
                </span>
              </div>
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
                      ? "已开启 WASAPI 独占直通 (--ao=wasapi --audio-exclusive=yes，占用时自动安全回退)"
                      : "已使用系统 WASAPI 共享模式 (点击开启独占直通)"}
                  </span>
                </button>
              </div>
            </div>

            <div className="settings-row">
              <div className="settings-label-block">
                <label>启用硬件加速（GPU 界面渲染与媒体解码加速）</label>
                <span className="settings-field-hint">
                  开启 GPU 光栅化与 AVX2 多线程硬解加速，显著降低高分屏与长音频播放时的 CPU 占用
                </span>
              </div>
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
                      ? "已开启全链路硬件加速 (WebView2 GPU + mpv 硬解/AVX2 多线程)"
                      : "已切换为纯软件兼容模式 (关闭硬件加速)"}
                  </span>
                </button>
              </div>
            </div>
          </div>
        </section>

        {/* 分组 3：在线源站、网络代理与远程挂载 */}
        <section
          className="settings-card settings-group-card"
          data-settings-group="network-online"
          aria-label="在线源站、网络代理与远程挂载"
        >
          <div className="settings-group-header">
            <div className="settings-group-title-wrap">
              <span className="settings-group-icon">
                <Globe size={16} />
              </span>
              <div>
                <h3>在线源站、网络代理与远程挂载</h3>
                <p className="settings-group-desc">
                  独立配置 asmr.one 镜像节点、HTTP/SOCKS5 网络代理、站点访问凭据及局域网 WebDAV/NAS 流式地址
                </p>
              </div>
            </div>
            <span className="badge badge-tech">网络与在线连接</span>
          </div>

          <div className="settings-group-body">
            <div className="settings-row">
              <div className="settings-label-block">
                <label htmlFor="setting-api-base">asmr.one API 基址</label>
                <span className="settings-field-hint">
                  用于在线浏览、元数据同步与云端字幕抓取的站点 API 域名，支持一键切换官方镜像
                </span>
              </div>
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
              <div className="settings-label-block">
                <label htmlFor="setting-proxy">
                  HTTP / SOCKS5 代理 (同时作用于 API 与 mpv)
                </label>
                <span className="settings-field-hint">
                  留空则直接连接；配置后将同步作用于在线接口请求、离线下载与 mpv 流式播放
                </span>
              </div>
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

            <div className="settings-row">
              <div className="settings-label-block">
                <label htmlFor="setting-token">
                  站点 Bearer 凭据 (存于 Windows 系统凭据管理器)
                </label>
                <span className="settings-field-hint">
                  令牌通过系统级 Keyring 加密存储，绝不明文写入数据库或日志
                </span>
              </div>
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
              <div className="settings-label-block">
                <label htmlFor="setting-webdav">
                  局域网 WebDAV / NAS 挂载流式地址
                </label>
                <span className="settings-field-hint">
                  可选挂载家庭 NAS 或 WebDAV 共享目录，支持跨局域网直接流式收听大容量无损作品
                </span>
              </div>
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
          </div>
        </section>

        {/* 分组 4：本地存储、目录实时监听与音频库根目录 */}
        <section
          className="settings-card settings-group-card"
          data-settings-group="storage-library"
          aria-label="本地存储、目录实时监听与音频库根目录"
        >
          <div className="settings-group-header">
            <div className="settings-group-title-wrap">
              <span className="settings-group-icon">
                <HardDrive size={16} />
              </span>
              <div>
                <h3>本地存储、目录实时监听与音频库根目录</h3>
                <p className="settings-group-desc">
                  统一管理离线下载存放目录、后台目录指纹增量监听以及外接硬盘盘符变更后的无损重定位
                </p>
              </div>
            </div>
            <span className="badge badge-tech">本地磁盘与目录树</span>
          </div>

          <div className="settings-group-body">
            <div className="settings-row">
              <div className="settings-label-block">
                <label htmlFor="setting-dl-dir">离线下载根目录</label>
                <span className="settings-field-hint">
                  在线作品下载完成后的本地存放根目录（下载中自动隔离为 .part 临时文件）
                </span>
              </div>
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
              <div className="settings-label-block">
                <label>目录实时文件监听与无感增量同步</label>
                <span className="settings-field-hint">
                  后台自动比对已导入根目录指纹，仅在新增或变更作品时触发秒级无感增量入库
                </span>
              </div>
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
                      ? "已开启后台目录实时监听与自动增量同步"
                      : "已暂停后台目录实时监听"}
                  </span>
                </button>
                <button
                  type="button"
                  className="btn btn-secondary btn-sm"
                  onClick={() => void handleSyncWatchedRootsNow()}
                >
                  <RefreshCw size={13} />
                  <span>立即增量同步监听目录</span>
                </button>
              </div>
            </div>

            <div className="settings-subsection">
              <div className="settings-subsection-title">
                已挂载的本地音频库根目录列表与盘符重定位
              </div>
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
          </div>
        </section>
      </form>

      <BackupSettings onRestored={loadAll} />

      <StorageSettings />
      <DiagnosticSettings />

      {/* 分组 6：脱敏诊断日志 */}
      <section
        className="settings-card settings-group-card"
        data-settings-group="cache-diagnostics"
        aria-label="脱敏诊断日志"
      >
        <div className="settings-group-header">
          <div className="settings-group-title-wrap">
            <span className="settings-group-icon">
              <Terminal size={16} />
            </span>
            <div>
              <h3>脱敏诊断日志</h3>
              <p className="settings-group-desc">
                查看已自动脱敏令牌的宿主运行日志
              </p>
            </div>
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
        <div className="log-box">
          <div className="log-header">
            <Terminal size={14} />
            <span>最近运行日志（已自动脱敏令牌与敏感参数）</span>
          </div>
          <pre className="log-pre">
            {logs.length === 0 ? "暂无日志记录" : logs.join("\n")}
          </pre>
        </div>
      </section>

      <ShortcutSettings smtcMode={settings?.smtcMode} />
    </div>
  );
};
