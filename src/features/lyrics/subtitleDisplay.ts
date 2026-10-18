export type SubtitleLanguageMode = "chinese" | "original" | "bilingual";

/** Existing subtitle cues carry newline/ // separated text, not per-line languages. */
export function subtitleLanguages(text: string) {
  const lines = text.split(/\r?\n|\s*\/\/\s*/).map(line => line.trim()).filter(Boolean);
  const kana = /[\u3040-\u30ff\uff66-\uff9f]/;
  const han = /\p{Script=Han}/u;
  const chinese = lines.filter(line => han.test(line) && !kana.test(line));
  const original = lines.filter(line => !chinese.includes(line));
  // Only separate languages when both are present; a wrapped monolingual cue
  // must remain intact, including Japanese lines consisting entirely of kanji.
  if (!chinese.length || !original.length) return { original: lines, chinese: lines, bilingual: lines };
  return { original, chinese, bilingual: lines };
}

export function subtitleLines(text: string, mode: SubtitleLanguageMode) {
  return subtitleLanguages(text)[mode];
}

export function preferredSubtitleLines(text: string, preference: "chinese" | "original") {
  const languages = subtitleLanguages(text);
  const primary = languages[preference].join(" ");
  const secondary = languages[preference === "chinese" ? "original" : "chinese"].join(" ");
  return [primary, secondary === primary ? "" : secondary];
}
