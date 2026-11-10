import type { EditableSubtitleDocument, SubtitleTaskDto } from "../../lib/recognition";

// The service currently reports phase strings, not typed units. Unknown phases
// stay generic rather than guessing that audio chunks are subtitle cues.
export function subtitleTaskUnit(phase: string) {
  if (phase === "按中文字幕时间段识别日语" || phase === "中文字幕对应段落已识别") return "参考时间段";
  if (phase.startsWith("识别原文") || phase === "当前预读已完成") return "音频片段";
  if (phase === "翻译字幕" || phase === "全文校对并翻译" || phase.startsWith("校对已保存，")) return "句";
  return "项";
}
export function subtitleTaskStatus(task: SubtitleTaskDto) {
  if (task.state === "failed") return "任务失败，已有结果已保留";
  if (task.state === "cancelled") return "已停止，已有结果已保留";
  if (task.state === "cancelling") return "正在停止当前片段…";
  if (task.state === "completed") return task.automatic ? "本次预读已结束" : task.completed < task.total ? "本次处理已结束，仍有未完成项" : "本次处理已完成";
  return "正在处理";
}
export function subtitleResultCounts(document: EditableSubtitleDocument) {
  const cues = document.cues.filter(cue => cue.status !== "noSpeech" && cue.status !== "nonDialogue");
  const failed = cues.filter(cue => cue.status === "translationFailed");
  const review = cues.filter(cue => cue.status === "audioReview" || !!cue.review);
  return { translated: cues.filter(cue => cue.translation.trim()).length, failed: failed.length,
    pending: cues.filter(cue => cue.status === "pendingTranslation").length,
    review: review.length, firstFailed: failed[0]?.index, firstReview: review[0]?.index };
}
