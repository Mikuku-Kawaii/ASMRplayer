export type AutoRecognitionMode = "off" | "missing" | "all";
export interface RecognitionSettingsDto {
  preferences?: SubtitlePreferencesDto;
  autoRecognition: AutoRecognitionMode;
  autoTranslation: boolean;
  preReadSeconds: number;
  translationProvider: "ollama" | "deepseek";
  ollamaUrl: string;
  ollamaModel: string;
  deepseekModel: string;
  deepseekPrompt?: string;
  fullTextReview?: boolean;
  chineseReference?: boolean;
  translationOptions?: TranslationOptions;
}
export interface TranslationOptions {
  ollamaThinking: "off" | "on" | "low" | "medium" | "high";
  deepseekThinking: "default" | "off" | "on";
  temperature: number;
  maxTokens: number;
}
export const defaultTranslationOptions: TranslationOptions = { ollamaThinking: "off", deepseekThinking: "default", temperature: 0.2, maxTokens: 2048 };
export interface SubtitleTerm { source: string; target: string; misrecognitions?: string[]; context?: string }
export interface SubtitleWorkContext { versionId: string; workId: string; title: string }
export interface SubtitlePreferencesDto {
  recognitionEngine: "sensevoice" | "sensevoice_fp16" | "whisper" | "faster_whisper" | "kotoba_whisper";
  recognitionContext: boolean;
  rerecognitionContext?: "standard" | "extended";
  language: string;
  translationStyle: string;
  translationPunctuation: string;
  glossary: SubtitleTerm[];
  workGlossaries: Record<string, SubtitleTerm[]>;
  correctionsEnabled: boolean;
  corrections: SubtitleTerm[];
}
export const defaultSubtitlePreferences: SubtitlePreferencesDto = {
  recognitionEngine: "sensevoice", recognitionContext: true, rerecognitionContext: "standard",
  language: "ja", translationStyle: "faithful", translationPunctuation: "standard",
  glossary: [], workGlossaries: {}, correctionsEnabled: false, corrections: [],
};
export interface ModelJobDto {
  state: "" | "running" | "cancelling" | "completed" | "cancelled" | "failed";
  label: string;
  completed: number;
  total: number | null;
  error: string | null;
  alternativeAvailable?: boolean;
  installedModel?: string | null;
}
export interface RecognitionSnapshotDto {
  modelDirectories?: Record<string, string>;
  ollamaDirectoryHint?: string;
  sensevoiceFp16Installed?: boolean;
  sensevoiceFp16Bytes?: number;
  whisperInstalled?: boolean;
  whisperBytes?: number;
  fasterWhisperInstalled?: boolean;
  fasterWhisperBytes?: number;
  kotobaWhisperInstalled?: boolean;
  kotobaWhisperBytes?: number;
  settings: RecognitionSettingsDto;
  hasDeepseekKey: boolean;
  recognitionInstalled: boolean;
  runtimeInstalled: boolean;
  runtimeBytes: number;
  recognitionBytes: number;
  recognitionDirectory: string;
  recognitionJob: ModelJobDto;
  translationJob: ModelJobDto;
  ollamaInstallJob?: ModelJobDto;
}
export interface AvailableModelDto { name: string; size: number | null }

export interface SubtitleTaskDto {
  automatic?: boolean;
  tracksCompleted?: number;
  tracksTotal?: number;
  reviewPending?: number;
  versionId: string | null;
  state: ModelJobDto["state"];
  phase: string;
  completed: number;
  total: number;
  error: string | null;
}
export interface EditableSubtitleCue {
  index: number; startMs: number; endMs: number;
  original: string; translation: string; edited: boolean;
  rawOriginal?: string; modelOriginal?: string | null;
  status?: "pendingTranslation" | "translationFailed" | "audioReview" | "noSpeech" | "nonDialogue" | "complete";
  canExcludeNonDialogue?: boolean;
  audioNote?: string | null;
  audioComparison?: string | null;
  review?: { note: string; contextFirstId: number; contextLastId: number; contextCount: number; totalCount: number } | null;
}
export interface EditableSubtitleDocument {
  versionId: string; sourceKey: string; revision: string;
  recognitionComplete: boolean; cues: EditableSubtitleCue[];
}
