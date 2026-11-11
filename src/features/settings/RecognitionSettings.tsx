import { SettingsSaveNote } from "./SettingsSaveNote";
import { ErrorNotice } from "../../components/ErrorNotice";
import { ContentDialog } from "../../components/ContentDialog";
import { useCallback, useEffect, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { Languages, Mic, RefreshCw } from "lucide-react";
import { HelpHint, SettingLabel } from "../../components/HelpHint";
import { Select } from "../../components/Select";
import { callCommand, formatBytes, normalizeAppError } from "../../lib/api";
import type { AutoRecognitionMode, AvailableModelDto, ModelJobDto, RecognitionSettingsDto, RecognitionSnapshotDto } from "../../lib/recognition";
import { defaultSubtitlePreferences, defaultTranslationOptions, type TranslationOptions } from "../../lib/recognition";
import { SettingsDisclosure } from "./SettingsDisclosure";
import { SubtitlePreferencesSettings } from "./SubtitlePreferencesSettings";

const active = (job: ModelJobDto) => job.state === "running" || job.state === "cancelling";
// V4.1 Flash is a display name; the official request ID remains deepseek-flash.
const deepseekPresets = [
  { value: "deepseek-flash", label: "DeepSeek V4.1 Flash（deepseek-flash）" },
  { value: "deepseek-v4-pro", label: "DeepSeek V4 Pro（deepseek-v4-pro）" },
];
function deepseekOptions(models: AvailableModelDto[], selected: string) {
  const options = [...deepseekPresets];
  const seen = new Set(options.map(option => option.value));
  for (const name of [...models.map(model => model.name), selected]) {
    if (name && !seen.has(name)) { options.push({ value: name, label: name }); seen.add(name); }
  }
  return options;
}
const modelCacheKey = (base: string) => `asmr-player:ollama-models:${base.trim().replace(/\/+$/, "")}`;
function cachedModels(base: string): AvailableModelDto[] | null {
  try {
    const value: unknown = JSON.parse(localStorage.getItem(modelCacheKey(base)) ?? "null");
    return Array.isArray(value) && value.every(model => typeof model?.name === "string" && (model.size === null || typeof model.size === "number")) ? value : null;
  } catch { return null; }
}
function ModelProgress({ job, name, cancel, disabled, cancelLabel = "取消下载" }: { job: ModelJobDto; name: string; cancel: () => void; disabled: boolean; cancelLabel?: string }) {
  if (!job.state) return null;
  return <div className="recognition-progress">
    <ErrorNotice message={job.error} />
    {!job.error && <p role="status">{job.label}</p>}
    {active(job) && <>
      <progress aria-label={`${name}下载进度`} max={job.total ?? 1} value={job.total ? Math.min(job.completed, job.total) : undefined} />
      <div className="recognition-progress-detail"><span>{formatBytes(job.completed)}{job.total ? ` / ${formatBytes(job.total)}` : ""}</span>
        <button type="button" className="btn btn-ghost btn-sm" disabled={disabled || job.state === "cancelling"} onClick={cancel}>{job.state === "cancelling" ? "正在取消…" : cancelLabel}</button></div>
    </>}
  </div>;
}

export function RecognitionSettings({ page = "all", onOpenGlossary }: { page?: "all" | "recognition" | "translation"; onOpenGlossary?: () => void }) {
  const [snapshot, setSnapshot] = useState<RecognitionSnapshotDto | null>(null);
  const [draft, setDraft] = useState<RecognitionSettingsDto | null>(null);
  const [key, setKey] = useState("");
  const [expandedProviders, setExpandedProviders] = useState<{ ollama: boolean; deepseek: boolean } | null>(null);
  const [ollamaModels, setOllamaModels] = useState<AvailableModelDto[] | null>(null);
  const [deepseekModels, setDeepseekModels] = useState<AvailableModelDto[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [pendingDelete, setPendingDelete] = useState<{engine: string; name: string; path: string; base?: string} | null>(null);
  const [promptExpanded, setPromptExpanded] = useState(false);
  const [pendingSave, setPendingSave] = useState<"key" | "settings" | null>(null);
  const mounted = useRef(false), locked = useRef(false), request = useRef(0);
  const lastImportedModel = useRef<string | null>(null);
  const observedTranslationJob = useRef(false);
  const rememberModels = useCallback((base: string, models: AvailableModelDto[]) => {
    try { localStorage.setItem(modelCacheKey(base), JSON.stringify(models)); } catch { /* Current session can still show the result. */ }
    setOllamaModels(models);
  }, []);
  const ollamaBase = draft?.ollamaUrl;
  useEffect(() => {
    setOllamaModels(ollamaBase ? cachedModels(ollamaBase) : null);
  }, [ollamaBase]);
  const invalidate = useCallback(() => { ++request.current; }, []);
  const refresh = useCallback(async () => {
    const id = ++request.current;
    const next = await callCommand<RecognitionSnapshotDto>("get_recognition_snapshot");
    if (mounted.current && id === request.current) { setSnapshot(next); setDraft(previous => previous ?? next.settings); }
  }, []);
  useEffect(() => {
    mounted.current = true;
    void refresh().catch(e => { if (mounted.current) setError(normalizeAppError(e).message); });
    return () => { mounted.current = false; invalidate(); };
  }, [refresh, invalidate]);
  const installing = !!snapshot?.ollamaInstallJob && active(snapshot.ollamaInstallJob);
  const downloading = !!snapshot && (active(snapshot.recognitionJob) || active(snapshot.translationJob) || installing);
  useEffect(() => {
    if (!downloading || busy) return;
    let pending = false;
    const timer = window.setInterval(() => {
      if (pending || locked.current) return;
      pending = true;
      void refresh().catch(e => { if (mounted.current) setError(normalizeAppError(e).message); }).finally(() => { pending = false; });
    }, 1000);
    return () => window.clearInterval(timer);
  }, [downloading, busy, refresh]);

  useEffect(() => {
    if (snapshot && active(snapshot.translationJob)) {
      observedTranslationJob.current = true;
      lastImportedModel.current = null;
      return;
    }
    const model = snapshot?.translationJob.installedModel;
    if (snapshot?.translationJob.state !== "completed" || !model || lastImportedModel.current === model) return;
    lastImportedModel.current = model;
    const cached = cachedModels(snapshot.settings.ollamaUrl);
    const models = cached ?? [];
    // A new completed task updates the cache; reopening must not undo a manual refresh.
    if (cached === null || observedTranslationJob.current) {
      rememberModels(snapshot.settings.ollamaUrl, models.some(item => item.name === model) ? models : [...models, { name: model, size: null }]);
    }
    observedTranslationJob.current = false;
    if (snapshot.settings.ollamaModel === model) {
      setDraft(previous => previous ? { ...previous, ollamaModel: model } : previous);
      setNotice("模型已就绪，并已选为本地翻译模型。");
    } else {
      setNotice("模型已就绪，可从已安装的本地模型中选择使用。");
    }
    window.dispatchEvent(new Event("subtitle-models-changed"));
  }, [snapshot, rememberModels]);

  async function run(action: () => Promise<void>) {
    if (locked.current) return;
    locked.current = true; ++request.current; setBusy(true); setError(""); setNotice("");
    try { await action(); await refresh(); window.dispatchEvent(new Event("subtitle-models-changed")); }
    catch (e) { if (mounted.current) setError(normalizeAppError(e).message); }
    finally { locked.current = false; if (mounted.current) setBusy(false); }
  }
  async function saveSettings() {
    if (!draft) return;
    const saved = await callCommand<RecognitionSettingsDto>("save_recognition_settings", { settings: draft });
    if (mounted.current) setDraft(saved);
  }
  async function saveLocalModelSelection() {
    if (!snapshot || !draft) return;
    // Download/import selects a local model, but must not silently save API or parameter drafts.
    await callCommand("save_recognition_settings", { settings: { ...snapshot.settings, ollamaUrl: draft.ollamaUrl, ollamaModel: draft.ollamaModel } });
  }
  async function checkOllama() {
    if (!draft) return;
    const models = await callCommand<AvailableModelDto[]>("check_ollama_connection", { base: draft.ollamaUrl });
    if (mounted.current) { rememberModels(draft.ollamaUrl, models); setNotice(`Ollama 已连接，已安装 ${models.length} 个模型。`); }
  }
  const cancel = (kind: string) => void run(async () => { await callCommand("cancel_model_download", { kind }); });
  const change = (patch: Partial<RecognitionSettingsDto>) => { setDraft(value => value ? { ...value, ...patch } : value); setNotice(""); };
  if (!snapshot || !draft) return <section className="settings-card settings-group-card" aria-label="识别与翻译">
    {error ? <><ErrorNotice message={error} /><button type="button" className="btn btn-secondary" disabled={busy} onClick={() => void run(refresh)}>重新加载</button></> : <p role="status">正在读取识别与翻译设置…</p>}
  </section>;
  const providerPanels = expandedProviders ?? { ollama: draft.translationProvider === "ollama", deepseek: draft.translationProvider === "deepseek" };
  const recognitionBusy = active(snapshot.recognitionJob);
  const translationBusy = active(snapshot.translationJob) || installing;
  const dirty = JSON.stringify(draft) !== JSON.stringify(snapshot.settings);
  const canAlternative = ["qwen3:4b", "qwen3:4b-q4_k_m"].includes(draft.ollamaModel.trim().toLowerCase());
  const selectedModel = ollamaModels?.find(model => model.name === draft.ollamaModel.trim() || model.name === `${draft.ollamaModel.trim()}:latest`);
  const translationInstalled = ollamaModels !== null
    ? !!selectedModel
    : snapshot.translationJob.state === "completed" && snapshot.translationJob.installedModel === draft.ollamaModel;
  const options = draft.translationOptions ?? defaultTranslationOptions;
  const changeOptions = (patch: Partial<TranslationOptions>) => change({ translationOptions: { ...options, ...patch } });
  const deleteButton = (engine: string, name: string, installed: boolean) => <button type="button" className="btn btn-ghost" disabled={busy || recognitionBusy || !installed} onClick={() => setPendingDelete({engine, name, path: snapshot.modelDirectories?.[engine] ?? (engine === "sensevoice" ? snapshot.recognitionDirectory : "应用模型目录")})}>删除模型</button>;
  const modelLocation = (engine: string) => <div className="settings-row"><label htmlFor={`model-path-${engine}`}>模型路径</label><input id={`model-path-${engine}`} className="input-text" readOnly value={snapshot.modelDirectories?.[engine] ?? (engine === "sensevoice" ? snapshot.recognitionDirectory : "读取中…")} /></div>;
  return <div className="recognition-settings" data-settings-group={page === "translation" ? "translation" : "recognition"} aria-busy={busy}>
    {pendingDelete && <ContentDialog size="compact" label="删除模型" busy={busy} onClose={() => setPendingDelete(null)}>
      <h3>删除 {pendingDelete.name}？</h3>
<div className="dialog-body">
      <p>{pendingDelete.engine === "ollama" ? "将通过 Ollama 删除此模型，其他使用它的应用也会受影响；原始 GGUF 文件保留。" : "删除此识别模型及同目录专用运行库，字幕和音频保留；再次使用需要重新下载。SenseVoice INT8 的独立运行库保留。"}</p>
      <p className="model-path-detail">{pendingDelete.path}</p><ErrorNotice message={error} />
      </div>
<div className="dialog-actions"><button type="button" className="btn btn-ghost" disabled={busy} onClick={() => setPendingDelete(null)}>取消</button>
      <button type="button" className="btn btn-danger btn-danger-solid" disabled={busy} onClick={() => void run(async () => {
        if (pendingDelete.engine === "ollama") {
          await callCommand("delete_ollama_model", {base: pendingDelete.base, model: pendingDelete.name});
          rememberModels(pendingDelete.base!, (ollamaModels ?? []).filter(model => model.name !== pendingDelete.name));
        } else { await callCommand("delete_recognition_model", {engine: pendingDelete.engine}); }
        setPendingDelete(null); setNotice("模型已删除，已有字幕保留。");
      })}>删除模型</button></div>
    </ContentDialog>}
    {pendingSave && <ContentDialog label="保存翻译配置" busy={busy} onClose={() => setPendingSave(null)}>
      <h3>保存前请确认</h3>
<div className="dialog-body">
      <ErrorNotice message={error} />
      {pendingSave === "key" || draft.translationProvider === "deepseek" ? <p>使用 DeepSeek 翻译时，会将字幕原文、上下文、相关词库及全局 Prompt 发送至 DeepSeek，并按 API 使用量计费。全文模式每批发送整轨原文及已有中文参考，会增加费用；开启思考也可能增加耗时和费用。自动翻译使用普通模式，播放时会自动调用。保存配置本身不会开始翻译。</p> : <p>本地翻译使用本机 Ollama，不产生 DeepSeek API 费用。全文模式使用更长上下文，思考和输出长度也会增加耗时及资源占用。保存配置不会下载模型或开始翻译。</p>}
      </div>
<div className="dialog-actions"><button type="button" className="btn btn-ghost" disabled={busy} onClick={() => setPendingSave(null)}>取消</button><button type="button" className="btn btn-primary" disabled={busy} onClick={() => void run(async () => {
        if (pendingSave === "key") {
          await callCommand("save_deepseek_key", { key });
          if (mounted.current) { setKey(""); setDeepseekModels([]); setNotice("API Key 已保存。"); }
        } else { await saveSettings(); if (mounted.current) setNotice("翻译设置已保存。"); }
        if (mounted.current) setPendingSave(null);
      })}>保存配置</button></div>
    </ContentDialog>}
    {!pendingSave && error && <ErrorNotice message={error} />}
    {notice && <p role="status">{notice}</p>}
    <section className="settings-card settings-group-card" data-settings-group="translation" hidden={page === "recognition"} aria-labelledby="automatic-translation-heading">
      <h3 id="automatic-translation-heading">自动翻译</h3>
      <SettingsSaveNote busy={busy} error={error}>自动保存 · 后续播放使用</SettingsSaveNote>
      <div className="settings-row automatic-subtitle-row"><SettingLabel htmlFor="automatic-translation" label="自动翻译" help={`播放时使用已保存的服务自动翻译；缺少原文时是否先识别，由自动识别选项决定。检测到中文内容则跳过。使用 DeepSeek 时会自动发送字幕文字并产生 API 费用。关闭后停止后续自动翻译。`} /><input id="automatic-translation" className="toggle-switch" type="checkbox" role="switch" checked={snapshot.settings.autoTranslation ?? false} disabled={busy} onChange={e => {
        const autoTranslation = e.target.checked;
        void run(async () => { await callCommand("save_recognition_settings", { settings: { ...snapshot.settings, autoTranslation } }); setDraft(value => value ? { ...value, autoTranslation } : value); });
      }} /></div>
    </section>
    <section className="settings-card settings-group-card" data-settings-group="translation" hidden={page === "recognition"} aria-labelledby="translation-heading">
      <h3 id="translation-heading"><Languages size={18} /><span className="ui-icon-label">字幕翻译</span></h3>
      <SettingsSaveNote dirty={dirty} busy={busy} error={error}>手动保存 · 下次翻译使用；已有译文需重新翻译。密钥单独保存。</SettingsSaveNote>
      <div className="settings-row"><SettingLabel htmlFor="translation-provider" label="翻译服务" help={`本地翻译使用本机模型；DeepSeek 翻译会将待翻译文字发送到 DeepSeek，并按其 API 规则计费。检测连接只读取模型列表；字幕文字仅在手动翻译或开启自动翻译后发送。`} /><Select id="translation-provider" disabled={busy || translationBusy} value={draft.translationProvider} onValueChange={value => { change({ translationProvider: value as RecognitionSettingsDto["translationProvider"] }); setExpandedProviders({ ollama: value === "ollama", deepseek: value === "deepseek" }); }} options={[{ value: "ollama", label: "本地 Ollama" }, { value: "deepseek", label: "DeepSeek API" }]} /></div>
      <SettingsDisclosure title="本地 Ollama" description="连接、模型下载与 GGUF 导入" summary={translationBusy ? "任务进行中" : draft.ollamaModel !== snapshot.settings.ollamaModel || draft.ollamaUrl !== snapshot.settings.ollamaUrl ? "未保存" : undefined} expanded={providerPanels.ollama} onExpandedChange={ollama => setExpandedProviders({ ...providerPanels, ollama })}>
      <div className="settings-row"><label htmlFor="ollama-url">Ollama 地址</label><input id="ollama-url" className="input-text" disabled={busy || translationBusy} value={draft.ollamaUrl} onChange={e => { change({ ollamaUrl: e.target.value }); setOllamaModels(null); }} spellCheck={false} /></div>
      <div className="settings-row"><div className="setting-label-help"><SettingLabel htmlFor="ollama-model" label="本地翻译模型" help={`点击「安装 Ollama」自动下载安装、启动并检测本地服务；已有安装会直接启动服务。就绪后可下载翻译模型，初始候选为 qwen3:4b。
已检测的模型信息会保留，点击刷新图标更新；重新进入设置不会自动连接服务。取消后保留 Ollama 已下载的分层，再次下载会继续。`} /><button type="button" className="btn-icon" aria-label="刷新本地模型信息" title="刷新本地模型信息" disabled={busy || translationBusy} onClick={() => void run(checkOllama)}><RefreshCw size={16} aria-hidden="true" /></button></div><input id="ollama-model" className="input-text" disabled={busy || translationBusy} value={draft.ollamaModel} onChange={e => change({ ollamaModel: e.target.value })} spellCheck={false} /></div>
      <div className="ollama-model-overview">
      <section className="ollama-model-info" aria-label="本地模型信息">
        <span className="ollama-model-info-label">当前模型</span>
        <strong>{draft.ollamaModel.trim() || "未选择模型"}</strong>
        <div className="ollama-model-info-meta">
          <span className="badge">{translationInstalled ? "已安装" : ollamaModels === null ? "尚未检测" : "未安装"}</span>
          <span>{selectedModel?.size ? formatBytes(selectedModel.size) : "大小未知"}</span>
        </div>
      </section>
      <div className="ollama-model-actions">
      <div className="recognition-actions">
        <button type="button" className="btn btn-secondary" disabled={busy || translationBusy} onClick={() => void run(async () => { await callCommand("install_ollama", { base: draft.ollamaUrl }); })}>{installing ? "正在安装 Ollama…" : "安装 Ollama"}</button>
        <button type="button" className="btn btn-secondary" disabled={busy || translationBusy} onClick={() => void run(checkOllama)}>检测本地服务</button>
        <button type="button" className="btn btn-secondary" disabled={busy || translationBusy || translationInstalled || !draft.ollamaModel.trim()} onClick={() => void run(async () => { await saveLocalModelSelection(); await callCommand("download_translation_model"); })}>{active(snapshot.translationJob) ? "正在准备模型…" : translationInstalled ? "模型已安装" : snapshot.translationJob.state === "failed" ? "重试下载翻译模型" : "下载翻译模型"}</button>
      </div>
      <div className="recognition-actions">
        <button type="button" className="btn btn-secondary" disabled={busy || translationBusy} onClick={() => void run(async () => {
          const path = await open({ multiple: false, directory: false, title: "选择单文件 GGUF 翻译模型", filters: [{ name: "GGUF 模型", extensions: ["gguf"] }] });
          if (typeof path !== "string") return;
          await saveLocalModelSelection();
          lastImportedModel.current = null;

          await callCommand("import_translation_gguf", { path });
        })}>导入本地 GGUF</button>
        <HelpHint label="导入本地 GGUF" text="选择 Ollama 支持的单文件对话模型。程序校验文件并导入本机 Ollama，成功后自动选用，保留原始文件且不覆盖已有模型。需额外模型存储空间；分片模型请先合并。取消后已导入的文件可能仍保留在 Ollama 中。" />
      </div>
      </div>
      </div>
      {snapshot.ollamaInstallJob?.state && <div className="recognition-progress">
        <ErrorNotice message={snapshot.ollamaInstallJob.error} />
        {!snapshot.ollamaInstallJob.error && <p role="status">{snapshot.ollamaInstallJob.label}</p>}
        {installing && <progress aria-label="Ollama 安装进度" max={snapshot.ollamaInstallJob.total ?? 1} value={snapshot.ollamaInstallJob.total ? snapshot.ollamaInstallJob.completed : undefined} />}
      </div>}
      {ollamaModels && <div className="settings-row"><label htmlFor="installed-translation-model">已安装的本地模型</label><Select id="installed-translation-model" value={draft.ollamaModel} disabled={busy || translationBusy || !ollamaModels.length} placeholder={ollamaModels.length ? "选择已安装模型" : "尚未安装模型"} options={ollamaModels.map(model => ({ value: model.name, label: `${model.name}${model.size ? ` · ${formatBytes(model.size)}` : ""}` }))} onValueChange={ollamaModel => change({ ollamaModel })} /></div>}
      {snapshot.translationJob.alternativeAvailable && <div className="recognition-progress">
        <p>替代下载使用 Qwen 官方 Qwen3 4B · Q4_K_M（约 2.5 GB），校验后导入 Ollama。支持断点续传，成功后自动选用；无需调整 Clash。</p>
        {!canAlternative && <p className="settings-field-hint">当前模型不在替代下载范围内。可改选 qwen3:4b，或导入对应的本地 GGUF。</p>}
        <button type="button" className="btn btn-secondary" disabled={busy || translationBusy || !canAlternative} onClick={() => void run(async () => {
          await saveLocalModelSelection();
          lastImportedModel.current = null;

          await callCommand("download_translation_model_alternative");
        })}>替代下载并导入</button>
      </div>}
      <ModelProgress job={snapshot.translationJob} name="翻译模型" disabled={busy} cancel={() => cancel("translation")} cancelLabel="取消任务" />
      <div className="settings-row"><SettingLabel htmlFor="ollama-model-path" label="模型路径" help="这是当前播放器环境推定的本机路径；Ollama 服务可能使用不同环境，暂不支持修改。删除交由 Ollama API 处理。" /><input id="ollama-model-path" className="input-text" readOnly value={snapshot.ollamaDirectoryHint ?? "由 Ollama 服务管理"} /></div>
      <div className="recognition-actions"><button type="button" className="btn btn-ghost" disabled={busy || translationBusy || !translationInstalled} onClick={() => setPendingDelete({engine:"ollama",name:selectedModel?.name ?? draft.ollamaModel,path:snapshot.ollamaDirectoryHint ?? "由 Ollama 服务管理",base:draft.ollamaUrl})}>删除本地翻译模型</button></div>
      </SettingsDisclosure>
      <SettingsDisclosure title="DeepSeek" description="API 密钥、连接检测与翻译模型" summary={key.trim() || draft.deepseekModel !== snapshot.settings.deepseekModel ? "未保存" : snapshot.hasDeepseekKey ? "密钥已保存" : "未设置密钥"} expanded={providerPanels.deepseek} onExpandedChange={deepseek => setExpandedProviders({ ...providerPanels, deepseek })}>
      <div className="settings-row"><SettingLabel htmlFor="deepseek-key" label="API Key" help={`官方接口 api.deepseek.com · API Key 仅保存到 Windows 凭据库，不写入普通设置或备份。`} /><input id="deepseek-key" className="input-text" type="password" autoComplete="off" spellCheck={false} disabled={busy} value={key} placeholder={snapshot.hasDeepseekKey ? "已保存，输入新密钥可替换" : "输入 DeepSeek API Key"} onChange={e => setKey(e.target.value)} /></div>
      <div className="recognition-actions">
        <button type="button" className="btn btn-secondary" disabled={busy || !key.trim()} onClick={() => setPendingSave("key")}>保存密钥</button>
        <button type="button" className="btn btn-secondary" disabled={busy || !snapshot.hasDeepseekKey || !!key.trim()} onClick={() => void run(async () => {
          const models = await callCommand<AvailableModelDto[]>("check_deepseek_connection");
          if (mounted.current) { setDeepseekModels(models); setDraft(value => value && ({ ...value, deepseekModel: value.deepseekModel || models[0]?.name || "" })); setNotice("DeepSeek 已连接，请选择模型并保存翻译设置。"); }
        })}>检测 DeepSeek 连接</button>
        <button type="button" className="btn btn-ghost" disabled={busy || !snapshot.hasDeepseekKey} onClick={() => void run(async () => { await callCommand("clear_deepseek_key"); if (mounted.current) { setKey(""); setDeepseekModels([]); setNotice("已移除保存的 API Key。"); } })}>移除密钥</button>
      </div>
      <div className="settings-row"><label htmlFor="deepseek-model">DeepSeek 模型</label><Select id="deepseek-model" disabled={busy} value={draft.deepseekModel} placeholder="选择 DeepSeek 模型" options={deepseekOptions(deepseekModels, draft.deepseekModel)} onValueChange={deepseekModel => change({ deepseekModel })} /></div>
      </SettingsDisclosure>
      <SettingsDisclosure title="翻译参数" description="全文校对、全局 Prompt、思考与输出长度" summary={dirty ? "有未保存修改" : "手动保存"}>
      <div className="settings-row"><SettingLabel htmlFor="full-text-review" label="全文校对并翻译" help="保存后手动点击翻译生效。在预算内发送全文，过长则按目标句与相邻原文分块，返回逐句候选校正及中文，保留编号与时间轴。识别尚未完成时需先完成识别；自动翻译仍使用普通模式。上下文更长，会增加耗时、显存和 API 费用；编辑字幕可查看待核对原因、分块范围及撤销模型校对；单句超过预算会提示，不截断句子。模型不能凭文字找回没识别出的声音。" /><input id="full-text-review" type="checkbox" className="toggle-switch" role="switch" checked={draft.fullTextReview ?? false} disabled={busy || translationBusy} onChange={e => change({fullTextReview:e.target.checked})} /></div>
      <SettingsDisclosure title="全局 Prompt" description="Ollama 与 DeepSeek 共用的称呼、语气和术语偏好" summary={draft.deepseekPrompt ? "已填写" : "使用默认规则"} expanded={promptExpanded} onExpandedChange={setPromptExpanded}>
        <label htmlFor="deepseek-prompt">附加翻译要求</label>
        <textarea id="deepseek-prompt" className="input-text deepseek-prompt" rows={6} maxLength={4000} disabled={busy || translationBusy} value={draft.deepseekPrompt ?? ""} placeholder="例如：称呼保持原文；使用自然中文口语，保留拟声词和拖长语气。" onChange={e => change({deepseekPrompt:e.target.value})} />
        <p className="muted">保存后用于 Ollama（含 qwen3:4b）与 DeepSeek。留空使用默认规则；完整覆盖和编号规则保留。已有译文需重新翻译。</p>
      </SettingsDisclosure>
        <div className="settings-row"><SettingLabel htmlFor="ollama-thinking" label="本地模型思考" help="Qwen3 通常使用关闭／开启；低、中、高仅适用于支持思考档位的模型，例如 GPT-OSS。具体能力以所选模型为准。不支持的档位可能被忽略或报错。思考可能明显增加翻译耗时。" /><Select id="ollama-thinking" disabled={busy || translationBusy} value={options.ollamaThinking} options={[{value:"off",label:"关闭（更快）"},{value:"on",label:"开启"},{value:"low",label:"低（需模型支持）"},{value:"medium",label:"中（需模型支持）"},{value:"high",label:"高（需模型支持）"}]} onValueChange={value => changeOptions({ollamaThinking:value as TranslationOptions["ollamaThinking"]})} /></div>
        <div className="settings-row"><SettingLabel htmlFor="deepseek-thinking" label="DeepSeek 思考" help="默认沿用模型行为。开启思考可能增加 token 用量和耗时，温度仅在明确关闭 DeepSeek 思考时发送；旧模型是否支持切换由服务端决定。" /><Select id="deepseek-thinking" disabled={busy || translationBusy} value={options.deepseekThinking} options={[{value:"default",label:"模型默认"},{value:"off",label:"关闭"},{value:"on",label:"开启"}]} onValueChange={value => changeOptions({deepseekThinking:value as TranslationOptions["deepseekThinking"]})} /></div>
        <div className="settings-row"><SettingLabel htmlFor="translation-temperature" label="温度" help="范围 0–2；较低值更稳定，较高值更多样。字幕翻译默认 0.2。适用于本地模型和关闭思考的 DeepSeek。" /><input id="translation-temperature" className="input-text" type="number" min={0} max={2} step={0.1} disabled={busy || translationBusy} value={Number.isFinite(options.temperature) ? options.temperature : ""} onChange={e => changeOptions({temperature:e.target.valueAsNumber})} /></div>
        <div className="settings-row"><SettingLabel htmlFor="translation-max-tokens" label="输出长度上限" help="每次请求最多生成的 token 数，范围 512–16384，默认 2048。思考模型可能将预算用于思考；截断时不会保存不完整译文，可提高上限后重试。" /><input id="translation-max-tokens" className="input-text" type="number" min={512} max={16384} step={512} disabled={busy || translationBusy} value={Number.isFinite(options.maxTokens) ? options.maxTokens : ""} onChange={e => changeOptions({maxTokens:e.target.valueAsNumber})} /></div>
      </SettingsDisclosure>
      <div className="recognition-actions settings-save-actions"><button type="button" className="btn btn-primary" disabled={busy || !dirty || translationBusy || !Number.isFinite(options.temperature) || options.temperature < 0 || options.temperature > 2 || !Number.isInteger(options.maxTokens) || options.maxTokens < 512 || options.maxTokens > 16384} onClick={() => setPendingSave("settings")}>保存翻译设置</button>{dirty && <span className="settings-field-hint">有未保存的修改</span>}</div>
    </section>
    <section className="settings-card settings-group-card" data-settings-group="recognition" hidden={page === "translation"} aria-labelledby="automatic-subtitle-heading">
      <h3 id="automatic-subtitle-heading">自动识别</h3>
      <SettingsSaveNote busy={busy} error={error}>自动保存 · 后续播放与自动任务使用；关闭后停止后续自动处理</SettingsSaveNote>
      <div className="settings-row"><SettingLabel htmlFor="automatic-recognition" label="自动识别" help={`仅无字幕时：已有中文字幕、日语字幕或其他字幕都跳过识别，未完成的识别缓存可继续。全自动识别：即使已有字幕，也补齐识别原文，保留原字幕文件。缺少识别模型或运行库时跳过，不自动下载。`} /><Select id="automatic-recognition" value={snapshot.settings.autoRecognition ?? "off"} disabled={busy} options={[{ value: "off", label: "不自动识别" }, { value: "missing", label: "仅无字幕时自动识别" }, { value: "all", label: "全自动识别" }]} onValueChange={value => {
        const autoRecognition = value as AutoRecognitionMode;
        void run(async () => { await callCommand("save_recognition_settings", { settings: { ...snapshot.settings, autoRecognition } }); setDraft(value => value ? { ...value, autoRecognition } : value); });
      }} /></div>
      <div className="settings-row"><SettingLabel htmlFor="recognition-pre-read" label="识别预读目标" help={`自动处理优先准备播放位置之后的字幕，按缓存块完成，实际预读可能略多；拖动进度后优先新位置。此值是目标，不保证实时速度。手动识别仍处理整条音轨。`} /><Select id="recognition-pre-read" value={String(snapshot.settings.preReadSeconds ?? 15)} disabled={busy} options={[10, 15, 20].map(seconds => ({ value: String(seconds), label: `${seconds} 秒` }))} onValueChange={value => {
        const preReadSeconds = Number(value);
        void run(async () => { await callCommand("save_recognition_settings", { settings: { ...snapshot.settings, preReadSeconds } }); setDraft(previous => previous ? { ...previous, preReadSeconds } : previous); });
      }} /></div>
    </section>
    <SubtitlePreferencesSettings page={page} onOpenGlossary={onOpenGlossary} initial={snapshot.settings.preferences ?? defaultSubtitlePreferences} busy={busy} run={run} onSaved={preferences => { setDraft(value => value ? { ...value, preferences } : value); }}>
      <div className="settings-row"><SettingLabel htmlFor="chinese-reference" label="按中文字幕时间段识别" help="开关立即保存，手动点击识别后生效。使用当前所选或匹配的带时间戳中文字幕，逐段识别日语并保留现有中文，以双语显示；参考模式只处理字幕覆盖的时间段。没有可用中文字幕时自动按普通方式识别整轨。新流程或更换参考字幕请选择重新识别。中文字幕不是日语识别提示词，也不会反译冒充原音。无时间戳台本需要另行音频对齐。" /><input id="chinese-reference" type="checkbox" className="toggle-switch" role="switch" checked={snapshot.settings.chineseReference ?? false} disabled={busy || recognitionBusy} onChange={e => {
        const chineseReference = e.target.checked;
        void run(async () => {
          await callCommand("save_recognition_settings", { settings: { ...snapshot.settings, chineseReference } });
          setDraft(value => value ? { ...value, chineseReference } : value);
        });
      }} /></div>
    </SubtitlePreferencesSettings>
    <section className="settings-card settings-group-card" data-settings-group="recognition" hidden={page === "translation"} aria-labelledby="recognition-model-heading">
      <h3 id="recognition-model-heading"><Mic size={18} /><span className="ui-icon-label">语音识别模型</span><HelpHint label="语音识别模型" text={`识别与翻译模型默认不下载。选择需要的模型后，手动下载。
识别结果按短句显示，不带标点。再次点击「识别」可整理旧的未翻译、未手工修正的识别字幕。`} /></h3>
      <SettingsDisclosure title="识别模型管理" description="下载、删除模型与安装运行库" summary={recognitionBusy ? "任务进行中" : undefined}>
      <div className="settings-nested-group">
      <div className="recognition-model-summary"><span className="setting-label-help"><strong>SenseVoiceSmall · INT8</strong><HelpHint label="SenseVoiceSmall 模型" text={`中文、日语、英语、韩语、粤语 · ${formatBytes(snapshot.recognitionBytes)} · 下载和导入均校验模型完整性。
导入同一版本的 model.int8.onnx 与 tokens.txt，保留原始文件。
保存位置：${snapshot.recognitionDirectory}`} /></span><span className="badge">{snapshot.recognitionInstalled ? "已安装" : "未安装"}</span></div>
      <div className="recognition-actions">
        <button type="button" className={`btn ${snapshot.recognitionInstalled ? "btn-secondary" : "btn-primary"}`} disabled={busy || recognitionBusy || snapshot.recognitionInstalled} onClick={() => void run(async () => { await callCommand("download_recognition_model"); })}>{recognitionBusy ? "正在准备模型…" : snapshot.recognitionInstalled ? "模型已安装" : snapshot.recognitionJob.state === "failed" ? "重试下载模型" : "下载识别模型"}</button>

        <button type="button" className="btn btn-secondary" disabled={busy || recognitionBusy || snapshot.runtimeInstalled} onClick={() => void run(async () => { await callCommand("download_recognition_runtime"); })}>{snapshot.runtimeInstalled ? "识别运行库已安装" : "下载识别运行库"}</button>
        {deleteButton("sensevoice", "SenseVoiceSmall INT8", snapshot.recognitionInstalled)}
        <HelpHint label="识别运行库" text={`sherpa-onnx 1.12.20 · ${formatBytes(snapshot.runtimeBytes)} · 首次识别需同时安装模型与运行库`} />
      </div>
      {modelLocation("sensevoice")}
      </div>

      <div className="settings-nested-group">
        <div className="recognition-model-summary"><span className="setting-label-help"><strong>SenseVoiceSmall · FP16 · GPU</strong><HelpHint label="SenseVoiceSmall FP16 模型" text={`使用 NVIDIA CUDA，下载量约 ${formatBytes(snapshot.sensevoiceFp16Bytes ?? 3101501219)}。下载官方浮点模型并在本地转换为 FP16，包含私有 CUDA 12、cuDNN 9 运行库；建议预留 12 GB 磁盘空间。取消保留下载进度。\n需要支持 CUDA 的 NVIDIA 显卡与驱动。GPU 初始化失败会提示错误，不自动切回 CPU。安装后在字幕偏好选择 FP16 并保存，已有字幕保留。`} /></span><span className="badge">{snapshot.sensevoiceFp16Installed ? "已安装" : "未安装"}</span></div>
        <div className="recognition-actions">
          <button type="button" className={`btn ${snapshot.sensevoiceFp16Installed ? "btn-secondary" : "btn-primary"}`} disabled={busy || recognitionBusy || snapshot.sensevoiceFp16Installed} onClick={() => void run(async () => { await callCommand("download_sensevoice_fp16"); })}>{snapshot.sensevoiceFp16Installed ? "FP16 模型已安装" : "下载 FP16 模型与 GPU 运行库"}</button>
          <button type="button" className="btn btn-secondary" disabled={busy || recognitionBusy || !snapshot.sensevoiceFp16Installed} onClick={() => void run(async () => { setNotice(await callCommand<string>("check_sensevoice_gpu")); })}>检测 GPU</button>
          {deleteButton("sensevoice_fp16", "SenseVoiceSmall FP16 GPU", !!snapshot.sensevoiceFp16Installed)}
        </div>
        {modelLocation("sensevoice_fp16")}
      </div>
      <div className="settings-nested-group">
      <div className="recognition-model-summary"><span className="setting-label-help"><strong>Whisper small · Q5_1</strong><HelpHint label="Whisper 模型" text={`本地多语言识别，支持人名术语上下文提示。模型与 CPU 运行库共 ${formatBytes(snapshot.whisperBytes ?? 193917919)}，下载后校验完整性。安装后在字幕偏好中选择 Whisper 并保存；不会自动切换引擎。`} /></span><span className="badge">{snapshot.whisperInstalled ? "已安装" : "未安装"}</span></div>
      <div className="recognition-actions"><button type="button" className={`btn ${snapshot.whisperInstalled ? "btn-secondary" : "btn-primary"}`} disabled={busy || recognitionBusy || snapshot.whisperInstalled} onClick={() => void run(async () => { await callCommand("download_whisper_model"); })}>{snapshot.whisperInstalled ? "Whisper 已安装" : "下载 Whisper 模型与运行库"}</button>{deleteButton("whisper", "Whisper small", !!snapshot.whisperInstalled)}</div>
      {modelLocation("whisper")}
      </div>
      {([
        { engine: "faster_whisper", title: "Faster-Whisper small", installed: snapshot.fasterWhisperInstalled, bytes: snapshot.fasterWhisperBytes },
        { engine: "kotoba_whisper", title: "Kotoba-Whisper v2.0（推荐）", installed: snapshot.kotobaWhisperInstalled, bytes: snapshot.kotobaWhisperBytes },
      ] as const).map(model => <div className="settings-nested-group" key={model.engine}>
        <div className="recognition-model-summary"><span className="setting-label-help"><strong>{model.title}</strong><HelpHint label={`${model.title} 模型`} text={`${model.engine === "kotoba_whisper" ? "日语专用模型，使用 Faster-Whisper 推理。" : "使用 CTranslate2 加速 Whisper small，支持词库提示。"}模型与独立运行库约 ${formatBytes(model.bytes ?? 0)}。默认 CPU INT8；已安装 SenseVoice GPU 运行库且 CUDA 检测通过时使用 GPU INT8/FP16。下载不会切换当前引擎，请在字幕偏好中选择并保存。`} /></span><span className="badge">{model.installed ? "已安装" : "未安装"}</span></div>
        <div className="recognition-actions">
          <button type="button" className={`btn ${model.installed ? "btn-secondary" : "btn-primary"}`} disabled={busy || recognitionBusy || model.installed} onClick={() => void run(async () => { await callCommand("download_ct2_model", { engine: model.engine }); })}>{model.installed ? "模型已安装" : "下载模型与运行库"}</button>
          <button type="button" className="btn btn-secondary" disabled={busy || recognitionBusy || !model.installed} onClick={() => void run(async () => { setNotice(await callCommand<string>("check_ct2_model", { engine: model.engine })); })}>检测模型</button>
          {deleteButton(model.engine, model.title, !!model.installed)}
        </div>
        {modelLocation(model.engine)}
      </div>)}
      <ModelProgress job={snapshot.recognitionJob} name="识别模型" disabled={busy} cancel={() => cancel("recognition")} />
      </SettingsDisclosure>
    </section>



  </div>;
}
