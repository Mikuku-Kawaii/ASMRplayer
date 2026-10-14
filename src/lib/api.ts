import { invoke } from "@tauri-apps/api/core";
import { listen, UnlistenFn } from "@tauri-apps/api/event";

export async function subscribeWindowMaximized(handler: (maximized: boolean) => void): Promise<UnlistenFn> {
  if (!isTauriRuntime()) return () => {};
  return listen<boolean>("window:maximized", event => handler(event.payload));
}

export interface AppError {
  code: string;
  message: string;
  retryable: boolean;
  hint?: string | null;
}

export interface UpdateSnapshot {
  eventSeq: number;
  phase: "idle" | "checking" | "up_to_date" | "available" | "downloading" | "verifying" | "ready" | "installing" | "error";
  currentVersion: string;
  latestVersion: string | null;
  notes: string | null;
  publishedAt?: string | null;
  checkedAt: string | null;
  downloadedBytes: number;
  totalBytes: number | null;
  autoCheck: boolean;
  skippedVersion: string | null;
  dismissed: boolean;
  error: AppError | null;
}

export async function subscribeUpdateSnapshot(handler: (snapshot: UpdateSnapshot) => void): Promise<UnlistenFn> {
  if (!isTauriRuntime()) return () => {};
  return listen<UpdateSnapshot>("updater:snapshot", (event) => handler(event.payload));
}

export interface Page<T> {
  items: T[];
  page: number;
  pageSize: number;
  totalCount: number;
}

export interface CollectionFilters {
  keyword: string;
  voiceActors: string[];
  tags: string[];
  collected: boolean | null;
  favorite: boolean | null;
  listening: "any" | "unplayed" | "inProgress" | "played";
  offline: boolean | null;
}

export interface SmartCollection {
  id: string;
  name: string;
  filters: CollectionFilters;
  updatedAt: string;
}

export interface OnlineFilter {
  kind: "voiceActor" | "circle" | "tag";
  value: string;
  exclude: boolean;
}

export interface ProviderTag {
  id: string;
  name: string;
  label: string;
  count: number;
}

export interface WorkSummary {
  listeningStatus?: "unplayed" | "inProgress" | "played";
  isListened?: boolean;
  id: string;
  title: string;
  rjCode?: string | null;
  circleName: string;
  voiceActors: string[];
  tags: string[];
  coverRef?: string | null;
  hasLocal: boolean;
  hasLocalImported?: boolean;
  hasOnline: boolean;
  hasDownloaded: boolean;
  availability: string;
  isCollected: boolean;
  isFavorite: boolean;
  lastVersionId?: string | null;
  lastTrackTitle?: string | null;
  lastPositionMs?: number | null;
  lastPlayedAt?: string | null;
  sourceWorkId?: string | null;
  updatedAt: string;
}

export type BulkAction = { kind: "collected" | "favorite" | "listened"; value: boolean } | { kind: "addTags" | "removeTags"; tags: string[] };
export interface BulkItem {
  workId: string;
  title: string;
  status: "ready" | "unchanged" | "failed" | "applied" | "undone" | "undoFailed";
  error: string | null;
}
export interface BulkReport {
  id: string;
  action: BulkAction;
  phase: "preview" | "applied" | "undone" | "undoPartial";
  items: BulkItem[];
}

export interface WorkOverrideDto {
  field: string;
  overrideMode: "set" | "cleared";
  valueText?: string | null;
}

export interface MediaSourceDto {
  id: string;
  versionId: string;
  sourceKind: "local_imported" | "local_downloaded" | "online_provider";
  provider?: string | null;
  sourceWorkId?: string | null;
  remoteFileId?: string | null;
  remoteRelativePath?: string | null;
  rootId?: string | null;
  relativePath?: string | null;
  absolutePath?: string | null;
  format: string;
  byteSize?: number | null;
  contentSha256?: string | null;
  availability: string;
}

export interface TrackVersionDto {
  isFavorite?: boolean;
  id: string;
  trackId: string;
  versionLabel: string;
  durationMs?: number | null;
  contentFingerprint?: string | null;
  associationState: string;
  positionMs: number;
  completed: boolean;
  sources: MediaSourceDto[];
}

export interface TrackItemDto {
  id: string;
  workId: string;
  title: string;
  logicalDirPath: string;
  groupLabel: string;
  sortKey: string;
  versions: TrackVersionDto[];
}

export interface AttachmentDto {
  id: string;
  workId: string;
  versionId?: string | null;
  attachmentType: "cover" | "subtitle" | "text";
  title: string;
  relativePath?: string | null;
  absolutePath?: string | null;
  remoteFileId?: string | null;
  subtitleOffsetMs: number;
  encoding: string;
  availability: string;
}

export interface WorkSourceInfoDto {
  id: string;
  provider: string;
  sourceWorkId: string;
  syncStatus: string;
  refreshedAt: string;
  expiresAt?: string | null;
  isExpired: boolean;
}

export interface DirectoryGroupOption {
  groupPath: string;
  label: string;
  trackCount: number;
  formats: string[];
}

export interface WorkDetailDto {
  id: string;
  title: string;
  description: string;
  rjCode?: string | null;
  circleName: string;
  voiceActors: string[];
  tags: string[];
  coverRef?: string | null;
  isCollected: boolean;
  isFavorite: boolean;
  overrides: WorkOverrideDto[];
  onlineSources: WorkSourceInfoDto[];
  tracks: TrackItemDto[];
  attachments: AttachmentDto[];
  directoryGroups: DirectoryGroupOption[];
  lastVersionId?: string | null;
  lastPositionMs?: number | null;
}

export interface LibraryRootDto {
  id: string;
  absolutePath: string;
  displayName: string;
  rootMode: string;
  availability: string;
  lastScannedAt?: string | null;
  workCount: number;
}

export interface ImportCandidateDto {
  relativeWorkPath: string;
  proposedTitle: string;
  detectedRjCode?: string | null;
  audioCount: number;
  subtitleCount: number;
  coverDetected: boolean;
  alreadyIndexed: boolean;
}

export interface ImportPreviewDto {
  rootPath: string;
  rootMode: string;
  overlappingRoots: string[];
  candidates: ImportCandidateDto[];
  selectedInputPath?: string | null;
  detectionReason?: string | null;
}

export interface ScanReportDto {
  rootId: string;
  worksAdded: number;
  worksUpdated: number;
  tracksAdded: number;
  corruptedFiles: string[];
  canceled: boolean;
}

export interface QueueItemDto {
  id: string;
  sortOrder: number;
  versionId: string;
  trackId: string;
  workId: string;
  workTitle: string;
  trackTitle: string;
  logicalDirPath: string;
  durationMs?: number | null;
  positionMs: number;
  completed: boolean;
  preferredSourceId?: string | null;
  hasLocal: boolean;
  hasOnline: boolean;
}

export interface AbLoopDto {
  id: string;
  versionId: string;
  startMs: number;
  endMs: number;
  label: string;
}

export interface BookmarkDto {
  id: string;
  versionId: string;
  trackTitle: string;
  workId: string;
  positionMs: number;
  note: string;
  createdAt: string;
  tags: string[];
  workTitle: string;
  versionLabel: string;
  linked: boolean;
  playable: boolean;
}

export interface BookmarkSearchPage { items: BookmarkDto[]; total: number }
export interface BookmarkBundle { schemaVersion: number; bookmarks: BackupBookmarkRecordDto[] }
export interface BookmarkRestoreReport { restored: number; unlinked: number; skipped: number }

export interface SleepTimerDto {
  mode: "off" | "minutes" | "end_of_track";
  deadlineUtc?: string | null;
  remainingSeconds?: number | null;
  targetGeneration?: number | null;
  fadeOutSeconds?: number;
}

export interface SubtitleCueDto {
  index: number;
  startMs: number;
  endMs: number;
  text: string;
}

export interface AutoMatchedSubtitleDto {
  attachment: AttachmentDto;
  cues: SubtitleCueDto[];
}

function normalizeSubtitleMatchStem(raw: string): string {
  let s = raw.trim().toLowerCase();
  for (let pass = 0; pass < 3; pass++) {
    const next = s.replace(
      /\.(lrc|srt|vtt|ass|ssa|txt|mp3|wav|flac|m4a|ogg|opus|aac|wma|mp4|mkv|webm)$/i,
      ""
    );
    if (next === s) break;
    s = next.trim();
  }
  for (let pass = 0; pass < 2; pass++) {
    const next = s.replace(
      /[._\-\s]*(zh-cn|zh-tw|zh-hk|zh|chs|cht|sc|tc|cn|jp|ja|en|ko|简|繁|中字|中文|日文|双语|字幕|台本)$/i,
      ""
    );
    if (next === s) break;
    s = next.trim();
  }
  return s.replace(/[\s_\-.[\]【】()（）「」『』~～!！?？:：,，、/\\]+/g, "");
}

function extractLeadingTrackIndex(raw: string): number | null {
  const m = raw
    .trim()
    .toLowerCase()
    .match(/^(?:track|tr|disc|cd|part|ep|ch|第)?[\s_\-.#]*(\d{1,4})/i);
  if (!m || !m[1]) return null;
  const n = Number.parseInt(m[1], 10);
  return Number.isFinite(n) ? n : null;
}

export function findBestSubtitleInAttachments(
  attachments: AttachmentDto[],
  currentVersionId?: string | null,
  currentTrackTitle?: string | null
): AttachmentDto | null {
  const subs = attachments.filter((a) => a.attachmentType === "subtitle");
  if (subs.length === 0) return null;
  if (currentVersionId) {
    const exactBound = subs.find((s) => s.versionId === currentVersionId);
    if (exactBound) return exactBound;
  }
  if (currentTrackTitle && currentTrackTitle.trim()) {
    const trkNorm = normalizeSubtitleMatchStem(currentTrackTitle);
    const trkIdx = extractLeadingTrackIndex(currentTrackTitle);
    let best: { score: number; att: AttachmentDto } | null = null;
    for (const sub of subs) {
      const subNorm = normalizeSubtitleMatchStem(sub.title);
      const subIdx = extractLeadingTrackIndex(sub.title);
      let score = 0;
      if (subNorm && subNorm === trkNorm) {
        score = 1000;
      } else if (
        subNorm.length >= 2 &&
        trkNorm.length >= 2 &&
        (subNorm.includes(trkNorm) || trkNorm.includes(subNorm))
      ) {
        if (subIdx !== null && trkIdx !== null && subIdx !== trkIdx) {
          score = 0;
        } else {
          score = 800;
        }
      } else if (subIdx !== null && trkIdx !== null && subIdx === trkIdx) {
        score = 600;
      }
      if (score > 0 && (!best || score > best.score)) {
        best = { score, att: sub };
      }
    }
    if (best) return best.att;
  }
  return subs.length === 1 ? subs[0] ?? null : null;
}

export interface PeqBandDto {
  enabled: boolean;
  filterType: "PK" | "LSC" | "HSC";
  frequencyHz: number;
  gainDb: number;
  qFactor: number;
}

export interface DspConfigDto {
  masterBypass: boolean;
  userPreampDb: number;
  autoeqPreampDb: number;
  hpfEnabled: boolean;
  hpfCutoffHz: number;
  autoeqEnabled: boolean;
  autoeqName?: string | null;
  autoeqBands: PeqBandDto[];
  deEsserEnabled: boolean;
  deEsserFreqHz: number;
  deEsserQ: number;
  deEsserGainDb: number;
  airEnabled: boolean;
  airFreqHz: number;
  airGainDb: number;
  balanceDb: number;
  swapChannels?: boolean;
  bs2bEnabled?: boolean;
  limiterEnabled: boolean;
  limiterThresholdDb: number;
  limiterLookaheadMs: number;
  limiterReleaseMs: number;
}

export interface TrackAcousticMetricsDto {
  versionId: string;
  integratedLufs: number;
  truePeakDbtp: number;
  loudnessRangeLra: number;
  dynamicRangeDr: number;
  waveformPeaks: number[];
}

export interface SmtcDisplayStateDto {
  enabled: boolean;
  mode: "full" | "privacy" | "disabled";
  playbackStatus: string;
  title: string;
  artist: string;
  albumTitle: string;
  coverPath?: string | null;
  positionMs: number;
  durationMs: number;
}

export interface DspPresetDto {
  id: string;
  name: string;
  description: string;
  config: DspConfigDto;
  isBuiltin: boolean;
}

export interface AudioStreamTelemetryDto {
  codec?: string | null;
  sampleRateHz?: number | null;
  sampleFormat?: string | null;
  channels?: string | null;
  bitrateKbps?: number | null;
  activeDspSummary: string;
  activeFilterGraph: string;
}

export interface DspStateDto {
  config: DspConfigDto;
  activePresetId?: string | null;
  workOverrideEnabled: boolean;
  workOverrideWorkId?: string | null;
  presets: DspPresetDto[];
  telemetry: AudioStreamTelemetryDto;
}

export interface PlaybackSnapshot {
  eventSeq: number;
  sessionGeneration: number;
  state: "idle" | "resolving" | "loading" | "playing" | "paused" | "buffering" | "ended" | "error";
  currentQueueItemId?: string | null;
  currentWorkId?: string | null;
  currentWorkTitle?: string | null;
  currentTrackId?: string | null;
  currentTrackTitle?: string | null;
  currentVersionId?: string | null;
  activeSource?: MediaSourceDto | null;
  alternativeSources: MediaSourceDto[];
  positionMs: number;
  durationMs?: number | null;
  volume: number;
  loopMode: "none" | "single" | "queue";
  activeAbLoop?: AbLoopDto | null;
  sleepTimer: SleepTimerDto;
  dspConfig: DspConfigDto;
  activeDspPresetId?: string | null;
  audioTelemetry: AudioStreamTelemetryDto;
  audioExclusive?: boolean;
  smtcMode?: "full" | "privacy" | "disabled";
  lastError?: AppError | null;
  queue: QueueItemDto[];
}

export interface PlaybackTickDto {
  eventSeq: number;
  sessionGeneration: number;
  positionMs: number;
  durationMs?: number | null;
  state: "idle" | "resolving" | "loading" | "playing" | "paused" | "buffering" | "ended" | "error";
  sleepRemainingSeconds?: number | null;
}

export interface DownloadPreviewItemDto {
  sourceId: string;
  versionId: string;
  trackTitle: string;
  format: string;
  byteSize?: number | null;
  targetRelativePath: string;
  alreadyDownloaded: boolean;
  alreadyQueued: boolean;
}

export interface DownloadPreviewDto {
  downloadRoot: string;
  workId: string;
  workTitle: string;
  items: DownloadPreviewItemDto[];
  totalBytes: number;
}

export interface DownloadTaskDto {
  priority: number;
  nextRetryAt?: string | null;
  id: string;
  workId: string;
  workTitle: string;
  versionId: string;
  trackTitle: string;
  sourceId: string;
  status: string;
  targetRelativePath: string;
  downloadedBytes: number;
  totalBytes?: number | null;
  speedBps: number;
  integrityMode: string;
  retryCount: number;
  errorCode?: string | null;
  errorMessage?: string | null;
  updatedAt: string;
}

export interface AppSettingsDto {
  downloadDir: string;
  apiBaseUrl: string;
  proxyUrl?: string | null;
  closeBehavior: "minimize_to_tray" | "quit";
  hardwareAcceleration: boolean;
  audioExclusive?: boolean;
  smtcMode?: "full" | "privacy" | "disabled";
  realtimeWatchEnabled?: boolean;
  webdavUrl?: string | null;
  nsfwEnabled?: boolean;
  islandFullscreenAvoidance?: boolean;
  islandEdgeSnap?: boolean;
  islandAlwaysOnTop?: boolean;
  islandScrollVolume?: boolean;
  islandToastEnabled?: boolean;
  islandToastDurationSec?: number;
  islandPrivacyMode?: boolean;
  hasAuthToken: boolean;
  mpvAvailable: boolean;
  mpvPath: string;
}

export interface IslandPlacementDto {
  x: number;
  y: number;
}

export type IslandForm = "compact" | "expanded" | "toast" | "menu";
export interface IslandLayout { revision: number; form: IslandForm; scale: number; width: number; height: number; radius?: number; animate?: boolean; nativeGlass?: boolean }
export interface IslandPreferences {
  compactScale: number;
  expandedScale: number;
  monitorId: string;
  activeMonitorId: string;
  monitors: { id: string; name: string; width: number; height: number; scaleFactor: number; primary: boolean }[];
  layout: IslandLayout;
}
export async function subscribeIslandLayout(handler: (value: IslandLayout) => void): Promise<UnlistenFn> {
  if (!isTauriRuntime()) return () => {};
  return listen<IslandLayout>("island:layout", e => handler(e.payload));
}
export async function subscribeActiveSubtitles(handler: (value: { eventSeq: number; versionId: string; cues: SubtitleCueDto[] }) => void): Promise<UnlistenFn> {
  if (!isTauriRuntime()) return () => {};
  return listen<{ eventSeq: number; versionId: string; cues: SubtitleCueDto[] }>("playback:subtitles", e => handler(e.payload));
}
export async function subscribeSettingsUpdated(handler: (value: AppSettingsDto) => void): Promise<UnlistenFn> {
  if (!isTauriRuntime()) return () => {};
  return listen<AppSettingsDto>("settings:updated", e => handler(e.payload));
}

export interface EncryptedSyncEnvelopeDto {
  version: number;
  kdfRounds: number;
  saltHex: string;
  nonceHex: string;
  ciphertextHex: string;
  macHex: string;
}

export interface BackupFavoriteRecordDto {
  workId: string;
  rjCode?: string | null;
  title: string;
}

export interface BackupPositionRecordDto {
  versionId: string;
  rjCode?: string | null;
  trackTitle: string;
  versionLabel: string;
  positionMs: number;
  durationMs?: number | null;
  completed: boolean;
  updatedAt: string;
}

export interface BackupBookmarkRecordDto {
  id: string;
  versionId: string;
  rjCode?: string | null;
  trackTitle: string;
  positionMs: number;
  note: string;
  createdAt: string;
  tags?: string[];
  workId?: string;
  workTitle?: string;
  versionLabel?: string;
  updatedAt?: string;
}

export interface BackupAbLoopRecordDto {
  id: string;
  versionId: string;
  rjCode?: string | null;
  trackTitle: string;
  startMs: number;
  endMs: number;
  label: string;
}

export interface BackupWorkOverrideRecordDto {
  workId: string;
  rjCode?: string | null;
  field: string;
  overrideMode: string;
  valueText?: string | null;
  updatedAt: string;
}

export interface BackupWorkDspOverrideRecordDto {
  workId: string;
  rjCode?: string | null;
  config: DspConfigDto;
  presetId?: string | null;
}

export interface UserBackupBundleDto {
  schemaVersion: number;
  exportedAt: string;
  favorites: BackupFavoriteRecordDto[];
  playbackPositions: BackupPositionRecordDto[];
  bookmarks: BackupBookmarkRecordDto[];
  abLoops: BackupAbLoopRecordDto[];
  workOverrides: BackupWorkOverrideRecordDto[];
  workDspOverrides: BackupWorkDspOverrideRecordDto[];
  customDspPresets: DspPresetDto[];
  dspGlobalConfig: DspConfigDto;
  dspGlobalPresetId: string | null;
}

export interface BackupRestoreReportDto {
  favoritesRestored: number;
  positionsRestored: number;
  bookmarksRestored: number;
  abLoopsRestored: number;
  overridesRestored: number;
  dspPresetsRestored: number;
}

export interface ProviderWork {
  sourceWorkId: string;
  rjCode?: string | null;
  title: string;
  circleName: string;
  voiceActors: string[];
  tags: string[];
  coverUrl?: string | null;
  durationSec?: number | null;
  hasSubtitle: boolean;
  nsfw?: boolean;
  releaseDate?: string | null;
  dlCount?: number | null;
  rateAverage2dp?: number | null;
  reviewCount?: number | null;
  price?: number | null;
}

export interface AssociationHistoryDto {
  id: string;
  entityType: string;
  primaryId: string;
  secondaryId: string;
  conflictNote?: string | null;
  undone: boolean;
  createdAt: string;
}

export function isTauriRuntime(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

export function normalizeAppError(err: unknown): AppError {
  if (err && typeof err === "object" && "code" in err && "message" in err) {
    return err as AppError;
  }
  if (typeof err === "string") {
    try {
      const parsed = JSON.parse(err) as unknown;
      if (parsed && typeof parsed === "object" && "code" in parsed) {
        return parsed as AppError;
      }
    } catch {
      // plain string
    }
    return {
      code: "INTERNAL_ERROR",
      message: err,
      retryable: false,
      hint: null,
    };
  }
  return {
    code: "INTERNAL_ERROR",
    message: err instanceof Error ? err.message : "发生未知错误",
    retryable: false,
    hint: null,
  };
}

export function formatDurationMs(ms?: number | null): string {
  if (ms === undefined || ms === null || ms < 0 || Number.isNaN(ms)) {
    return "00:00";
  }
  const totalSec = Math.floor(ms / 1000);
  const h = Math.floor(totalSec / 3600);
  const m = Math.floor((totalSec % 3600) / 60);
  const s = totalSec % 60;
  const pad = (n: number) => String(n).padStart(2, "0");
  return h > 0 ? `${pad(h)}:${pad(m)}:${pad(s)}` : `${pad(m)}:${pad(s)}`;
}

export function formatBytes(bytes?: number | null): string {
  if (bytes === undefined || bytes === null || bytes < 0) {
    return "未知大小";
  }
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
}

export const defaultDspConfig: DspConfigDto = {
  masterBypass: false,
  userPreampDb: 0,
  autoeqPreampDb: 0,
  hpfEnabled: true,
  hpfCutoffHz: 50,
  autoeqEnabled: false,
  autoeqName: null,
  autoeqBands: [],
  deEsserEnabled: false,
  deEsserFreqHz: 6500,
  deEsserQ: 4.0,
  deEsserGainDb: -3.5,
  airEnabled: false,
  airFreqHz: 12000,
  airGainDb: 1.5,
  balanceDb: 0,
  swapChannels: false,
  bs2bEnabled: false,
  limiterEnabled: true,
  limiterThresholdDb: -1.0,
  limiterLookaheadMs: 4.0,
  limiterReleaseMs: 80,
};

export const builtinDspPresets: DspPresetDto[] = [
  {
    id: "bypass",
    name: "原声直通 (Bit-Perfect)",
    description: "不做任何滤波与压限，100% 还原双声道录音原貌",
    isBuiltin: true,
    config: {
      ...defaultDspConfig,
      masterBypass: true,
      hpfEnabled: false,
      deEsserEnabled: false,
      airEnabled: false,
      limiterEnabled: false,
    },
  },
  {
    id: "asmr_pure_guard",
    name: "纯净防炸耳 (推荐日常)",
    description: "滤除 50Hz 以下麦克风震动并开启 -1.0 dBFS 4ms 前瞻防炸耳保护，不改变人声音色",
    isBuiltin: true,
    config: { ...defaultDspConfig },
  },
  {
    id: "asmr_de_esser",
    name: "柔和去齿音 (口腔音/掏耳)",
    description: "针对高刺激咀嚼音、舌音、竹签敲击，精准衰减 6.5 kHz 尖锐瞬态，降低听觉疲劳",
    isBuiltin: true,
    config: {
      ...defaultDspConfig,
      hpfEnabled: true,
      hpfCutoffHz: 55,
      deEsserEnabled: true,
      deEsserFreqHz: 6500,
      deEsserQ: 4.0,
      deEsserGainDb: -4.5,
      limiterEnabled: true,
      limiterThresholdDb: -1.5,
    },
  },
  {
    id: "asmr_whisper_sleep",
    name: "贴耳助眠耳语 (夜间深度)",
    description: "增强 12 kHz 呼吸空气感、柔化 6.5 kHz 齿音，并将防炸耳天花板收紧至 -3.0 dBFS 安心入睡",
    isBuiltin: true,
    config: {
      ...defaultDspConfig,
      hpfEnabled: true,
      hpfCutoffHz: 50,
      deEsserEnabled: true,
      deEsserFreqHz: 6500,
      deEsserQ: 4.0,
      deEsserGainDb: -3.0,
      airEnabled: true,
      airFreqHz: 12000,
      airGainDb: 1.5,
      limiterEnabled: true,
      limiterThresholdDb: -3.0,
      limiterLookaheadMs: 4.5,
      limiterReleaseMs: 90,
    },
  },
];

function peakingResponseDb(f: number, fc: number, gainDb: number, q: number): number {
  if (Math.abs(gainDb) < 0.01 || fc <= 0 || q <= 0) return 0;
  const x = (f / fc - fc / f) * q;
  return gainDb / (1 + x * x);
}

function shelfResponseDb(f: number, fc: number, gainDb: number, isHighShelf: boolean): number {
  if (Math.abs(gainDb) < 0.01 || fc <= 0) return 0;
  const ratio = isHighShelf ? f / fc : fc / f;
  const r2 = ratio * ratio;
  const weight = (r2 * r2) / (1 + r2 * r2);
  return gainDb * weight;
}

export function computeFrequencyResponseCurve(
  config: DspConfigDto,
  freqsHz: number[]
): number[] {
  if (config.masterBypass) {
    return freqsHz.map(() => 0);
  }
  return freqsHz.map((f) => {
    let db = 0;
    if (config.hpfEnabled && config.hpfCutoffHz > 0) {
      const ratio = config.hpfCutoffHz / Math.max(f, 1);
      const r4 = ratio * ratio * ratio * ratio;
      db += -10 * Math.log10(1 + r4);
    }
    if (config.autoeqEnabled) {
      for (const band of config.autoeqBands) {
        if (!band.enabled) continue;
        if (band.filterType === "PK") {
          db += peakingResponseDb(f, band.frequencyHz, band.gainDb, band.qFactor);
        } else if (band.filterType === "LSC") {
          db += shelfResponseDb(f, band.frequencyHz, band.gainDb, false);
        } else if (band.filterType === "HSC") {
          db += shelfResponseDb(f, band.frequencyHz, band.gainDb, true);
        }
      }
    }
    if (config.deEsserEnabled) {
      db += peakingResponseDb(f, config.deEsserFreqHz, config.deEsserGainDb, config.deEsserQ);
    }
    if (config.airEnabled) {
      db += shelfResponseDb(f, config.airFreqHz, config.airGainDb, true);
    }
    return Math.max(-18, Math.min(12, db));
  });
}

export const defaultSnapshot: PlaybackSnapshot = {
  eventSeq: 0,
  sessionGeneration: 1,
  state: "idle",
  currentQueueItemId: null,
  currentWorkId: null,
  currentWorkTitle: null,
  currentTrackId: null,
  currentTrackTitle: null,
  currentVersionId: null,
  activeSource: null,
  alternativeSources: [],
  positionMs: 0,
  durationMs: null,
  volume: 100,
  loopMode: "none",
  activeAbLoop: null,
  sleepTimer: {
    mode: "off",
    deadlineUtc: null,
    remainingSeconds: null,
    targetGeneration: null,
    fadeOutSeconds: 30,
  },
  dspConfig: { ...defaultDspConfig },
  activeDspPresetId: "asmr_pure_guard",
  audioTelemetry: {
    codec: null,
    sampleRateHz: null,
    sampleFormat: null,
    channels: null,
    bitrateKbps: null,
    activeDspSummary: "HPF 50Hz · 防炸耳 -1.0dB",
    activeFilterGraph: "",
  },
  audioExclusive: false,
  smtcMode: "full",
  lastError: null,
  queue: [],
};

export async function callCommand<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (!isTauriRuntime()) {
    throw {
      code: "IPC_UNAVAILABLE",
      message: `当前运行在非 Tauri 浏览器测试环境 (${cmd})`,
      retryable: false,
    } satisfies AppError;
  }
  try {
    return await invoke<T>(cmd, args);
  } catch (e) {
    throw normalizeAppError(e);
  }
}

export async function subscribePlaybackSnapshot(
  handler: (snap: PlaybackSnapshot) => void
): Promise<UnlistenFn> {
  if (!isTauriRuntime()) {
    return () => {};
  }
  return listen<PlaybackSnapshot>("playback:snapshot", (ev) => {
    handler(ev.payload);
  });
}

export async function subscribePlaybackTick(
  handler: (tick: PlaybackTickDto) => void
): Promise<UnlistenFn> {
  if (!isTauriRuntime()) {
    return () => {};
  }
  return listen<PlaybackTickDto>("playback:tick", (ev) => {
    handler(ev.payload);
  });
}

export async function subscribeDownloadsUpdated(
  handler: (tasks: DownloadTaskDto[]) => void
): Promise<UnlistenFn> {
  if (!isTauriRuntime()) {
    return () => {};
  }
  return listen<DownloadTaskDto[]>("downloads:updated", (ev) => {
    handler(ev.payload);
  });
}

export async function subscribeIslandVisibility(
  handler: (isOpen: boolean) => void
): Promise<UnlistenFn> {
  if (!isTauriRuntime()) {
    return () => {};
  }
  return listen<boolean>("island:visibility", (ev) => {
    handler(ev.payload);
  });
}

export async function subscribeIslandFullscreen(
  handler: (isFullscreen: boolean) => void
): Promise<UnlistenFn> {
  if (!isTauriRuntime()) {
    return () => {};
  }
  return listen<boolean>("island:fullscreen", (ev) => {
    handler(ev.payload);
  });
}

export function isDesktopIslandWindow(): boolean {
  if (typeof window === "undefined") return false;
  if (
    window.location.search.includes("window=island") ||
    window.location.hash.includes("island")
  ) {
    return true;
  }
  try {
    const internals = window as unknown as {
      __TAURI_INTERNALS__?: { metadata?: { currentWebview?: { label?: string } } };
    };
    if (internals?.__TAURI_INTERNALS__?.metadata?.currentWebview?.label === "island") {
      return true;
    }
  } catch {
    // fallback
  }
  return false;
}


export async function subscribeIslandGlass(handler: (enabled: boolean) => void): Promise<UnlistenFn> {
  if (!isTauriRuntime()) return () => {};
  return listen<boolean>("island:glass", e => handler(e.payload));
}

export interface AudioOutputStateDto {
  devices: { id: string; name: string }[];
  selectedDevice: string;
}
export interface QueuePlanDto { id: string; name: string; itemCount: number; updatedAt: string }
export interface QueuePlanEntryDto {
  versionId: string; preferredSourceId: string | null; workTitle: string;
  trackTitle: string; versionLabel: string; unavailableReason: string | null;
}
export interface QueuePlanPreviewDto {
  plan: QueuePlanDto; items: QueuePlanEntryDto[]; previewToken: string;
  queueToken: string; currentQueueCount: number;
}

export interface MediaAffinity {
  workId: string; versionId: string; isCollected: boolean; isFavorite: boolean;
  isVersionFavorite: boolean; isLocalImported: boolean;
}
export async function subscribeLibraryAffinity(handler: (workId: string) => void): Promise<UnlistenFn> {
  if (!isTauriRuntime()) return () => {};
  return listen<string>("library:affinity", e => handler(e.payload));
}
export interface DesktopLyricsSettingsDto {
  enabled: boolean; locked: boolean; fontSize: number; color: "lavender" | "white" | "amber";
  fontFamily: "system" | "yahei" | "dengxian" | "simsun" | "kaiti" | "meiryo";
  backgroundEnabled: boolean; backgroundOpacity: number; showTranslation: boolean; revision: number;
}
export async function subscribeDesktopLyricsSettings(handler: (s: DesktopLyricsSettingsDto) => void): Promise<UnlistenFn> {
  if (!isTauriRuntime()) return () => {};
  return listen<DesktopLyricsSettingsDto>("desktop-lyrics:settings", e => handler(e.payload));
}
