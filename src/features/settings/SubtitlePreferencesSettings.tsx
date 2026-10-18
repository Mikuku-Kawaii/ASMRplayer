import { SettingsDisclosure } from "./SettingsDisclosure";
import { useEffect, useState } from "react";
import { SettingLabel } from "../../components/HelpHint";
import { Select } from "../../components/Select";
import { callCommand } from "../../lib/api";
import type { SubtitlePreferencesDto, SubtitleTerm, SubtitleWorkContext } from "../../lib/recognition";

const printTerms = (terms: SubtitleTerm[]) => terms.map(term => `${term.source} => ${term.target}${term.misrecognitions?.length || term.context ? ` | ${(term.misrecognitions ?? []).join(" / ")} | ${term.context ?? ""}` : ""}`).join("\n");
export function parseTerms(text: string): SubtitleTerm[] {
  return text.split("\n").flatMap((line, index) => {
    if (!line.trim()) return [];
    const separator = line.indexOf("=>");
    const source = line.slice(0, separator).trim();
    const [target, aliases = "", context = "", ...extra] = line.slice(separator + 2).split("|").map(part => part.trim());
    if (extra.length) throw new Error(`第 ${index + 1} 行语境说明不能包含 |。`);
    if (separator < 1 || !source || !target) throw new Error(`第 ${index + 1} 行请使用「原文 => 替换文字」格式。`);
    return [{ source, target, ...(aliases || context ? { misrecognitions: aliases.split("/").map(form => form.trim()).filter(Boolean), context } : {}) }];
  });
}

export function SubtitlePreferencesSettings({ initial, busy, run, onSaved }: {
  initial: SubtitlePreferencesDto; busy: boolean; run: (action: () => Promise<void>) => Promise<void>;
  onSaved: (value: SubtitlePreferencesDto) => void;
}) {
  const [draft, setDraft] = useState(initial);
  const [glossary, setGlossary] = useState(printTerms(initial.glossary));
  const [rules, setRules] = useState(printTerms(initial.corrections));
  const [workTexts, setWorkTexts] = useState<Record<string, string>>({});
  const [work, setWork] = useState<SubtitleWorkContext | null>(null);
  const [notice, setNotice] = useState("");
  const serialized = JSON.stringify(initial);
  useEffect(() => {
    const value: SubtitlePreferencesDto = JSON.parse(serialized);
    setDraft(value); setGlossary(printTerms(value.glossary)); setRules(printTerms(value.corrections));
    setWorkTexts(Object.fromEntries(Object.entries(value.workGlossaries).map(([id, entries]) => [id, printTerms(entries)])));
  }, [serialized]);
  const change = (patch: Partial<SubtitlePreferencesDto>) => { setDraft(value => ({ ...value, ...patch })); setNotice(""); };
  const dirty = JSON.stringify(draft) !== serialized || glossary !== printTerms(initial.glossary) || rules !== printTerms(initial.corrections)
    || Object.entries(workTexts).some(([id, text]) => text !== printTerms(initial.workGlossaries[id] ?? []));
  async function save() {
    const workGlossaries = Object.fromEntries(Object.entries(workTexts).map(([id, text]) => [id, parseTerms(text)] as const).filter(([, entries]) => entries.length));
    const value = await callCommand<SubtitlePreferencesDto>("save_subtitle_preferences", { preferences: { ...draft, glossary: parseTerms(glossary), corrections: parseTerms(rules), workGlossaries } });
    onSaved(value); setNotice("字幕偏好已保存。新任务使用这些偏好，已有原文保留。");
  }
  function apply(undo: boolean) {
    void run(async () => {
      setNotice("");
      const context = await callCommand<SubtitleWorkContext>("get_subtitle_work_context");
      const count = await callCommand<number>("apply_subtitle_correction_rules", { versionId: context.versionId, undo });
      setNotice(undo ? `已撤销规则，恢复 ${count} 句原文；本音轨暂停套用规则，直到再次点击应用。` : `已更新 ${count} 句字幕，手工修正保留。`);
    });
  }
  return <section className="settings-card settings-group-card subtitle-preferences" aria-labelledby="subtitle-preferences-heading">
    <h3 id="subtitle-preferences-heading">字幕偏好</h3>
    <div className="settings-row"><SettingLabel htmlFor="subtitle-engine" label="识别引擎" help={`SenseVoice 无台词片段中识别出的呼气、笑声、咳嗽等声音按识别语言显示拟声词，日语使用「ふう」「ふふ」「こほん」，中文使用「呼」「呵呵」「咳咳」；Whisper 系列不提供这些声音标签。连续重复拟声词最多保留 3 次，并与正文分行显示。轻声对白保留识别文字。切换引擎或词库仅影响尚未识别的片段，已有字幕和手工修正保留。`} /><Select id="subtitle-engine" disabled={busy} value={draft.recognitionEngine ?? "sensevoice"} onValueChange={value => change({ recognitionEngine: value as SubtitlePreferencesDto["recognitionEngine"], ...(value === "kotoba_whisper" ? { language: "ja" } : value.includes("whisper") && draft.language === "yue" ? { language: "auto" } : {}) })} options={[{ value: "sensevoice", label: "SenseVoiceSmall · INT8 · CPU" }, { value: "sensevoice_fp16", label: "SenseVoiceSmall · FP16 · GPU" }, { value: "whisper", label: "Whisper small · CPU" }, { value: "faster_whisper", label: "Faster-Whisper small" }, { value: "kotoba_whisper", label: "Kotoba-Whisper v2.0 · 日语" }]} /></div>
    <div className="settings-row automatic-subtitle-row"><SettingLabel htmlFor="subtitle-context" label="识别使用词库提示" help={`Whisper 仅使用词库的正确词辅助识别人名与术语，不传误识形式和语境说明。只填人名、称谓和术语，翻译风格请填写翻译参数中的全局 Prompt。优先使用作品词条，最多 32 条；词库过长时只使用前面的完整词条。提示不能保证命中，也不会强制替换台词。选择引擎和保存偏好不会下载模型。`} /><input id="subtitle-context" className="toggle-switch" type="checkbox" role="switch" checked={draft.recognitionContext ?? true} disabled={busy || !draft.recognitionEngine.includes("whisper")} onChange={e => change({ recognitionContext: e.target.checked })} /></div>
    <div className="settings-row"><SettingLabel htmlFor="subtitle-language" label="识别语言" help={`默认使用日语并固定模型语言，不逐句切换；日语模式下独立的「嗯」等短应答按日语表达，保留正常日语汉字。自动判断优先参考较长音频片段，短应答证据不足时参考作品大标题中的假名或韩文；纯汉字标题不直接推断语言。已保存的语言选择保留，增量识别可整理未编辑、未翻译的旧拟声词与重复碎片；已有完整台词需重新识别才会更新。`} /><Select id="subtitle-language" disabled={busy} value={draft.language} onValueChange={language => change({ language })} options={[{ value: "ja", label: "日语" }, { value: "auto", label: "自动判断" }, { value: "zh", label: "普通话" }, { value: "yue", label: "粤语" }, { value: "en", label: "英语" }, { value: "ko", label: "韩语" }].filter(option => draft.recognitionEngine === "kotoba_whisper" ? ["ja", "auto"].includes(option.value) : !draft.recognitionEngine.includes("whisper") || option.value !== "yue")} /></div>
    <div className="settings-row"><SettingLabel htmlFor="subtitle-voice-profile" label="复识别上下文" help="标准前后各最多 2 秒，扩展前后各最多 8 秒；单次模型输入总长最多 20 秒。用于疑似片段的复识别及非台词复核，保留核心区独立对照，邻句不会直接挪入当前字幕。扩展模式耗时更长；保存后点击增量识别可复核已有候选。" /><Select id="subtitle-voice-profile" disabled={busy} value={draft.quietSpeech ? "quiet" : "normal"} onValueChange={value => change({ quietSpeech: value === "quiet" })} options={[{ value: "normal", label: "标准（前后各 2 秒）" }, { value: "quiet", label: "扩展（前后各 8 秒）" }]} /></div>
    <div className="settings-row"><SettingLabel htmlFor="subtitle-translation-style" label="译文风格" help={`风格与标点仅影响译文，不补写台词。修改词库或译文偏好后，下次翻译会更新自动译文，手工译文保留。`} /><Select id="subtitle-translation-style" disabled={busy} value={draft.translationStyle} onValueChange={translationStyle => change({ translationStyle })} options={[{ value: "faithful", label: "忠实" }, { value: "natural", label: "自然" }, { value: "soft", label: "柔和" }]} /></div>
    <div className="settings-row"><SettingLabel htmlFor="subtitle-translation-punctuation" label="译文标点" help={`风格与标点仅影响译文，不补写台词。修改词库或译文偏好后，下次翻译会更新自动译文，手工译文保留。`} /><Select id="subtitle-translation-punctuation" disabled={busy} value={draft.translationPunctuation} onValueChange={translationPunctuation => change({ translationPunctuation })} options={[{ value: "standard", label: "普通标点" }, { value: "minimal", label: "不带标点" }, { value: "soft", label: "适量使用 ～" }]} /></div>
    <SettingsDisclosure title="人名与术语词库" description="编辑全局词条与当前作品词条" summary={glossary !== printTerms(initial.glossary) || Object.entries(workTexts).some(([id, text]) => text !== printTerms(initial.workGlossaries[id] ?? [])) ? "未保存" : `${initial.glossary.length} 条全局词条`}>
    <div className="subtitle-preference-field"><SettingLabel htmlFor="subtitle-glossary" label="全局人名术语词库" help={`每行「正确词 => 译名 | 常见误识 / 其他误识 | 适用语境」，后两项可省略。误识形式仅用于检索候选，不会自动替换。同一原文以作品词库为准；合并后最多 100 条、6000 字节。只有正确词传给 Whisper 识别提示，译名、误识形式和语境仅供翻译校对；SenseVoice 不支持词库提示。不保证模型逐条采纳。`} /><textarea id="subtitle-glossary" className="input-text" rows={4} disabled={busy} value={glossary} onChange={e => { setGlossary(e.target.value); setNotice(""); }} placeholder="先輩 => 前辈" /></div>
    <div className="recognition-actions"><button type="button" className="btn btn-secondary" disabled={busy} onClick={() => void run(async () => { setNotice(""); setWork(await callCommand<SubtitleWorkContext>("get_subtitle_work_context")); })}>编辑当前作品词库</button></div>
    {work && <label className="subtitle-preference-field">作品词库 · {work.title}<textarea className="input-text" rows={4} disabled={busy} value={workTexts[work.workId] ?? ""} onChange={e => { setWorkTexts(value => ({ ...value, [work.workId]: e.target.value })); setNotice(""); }} /></label>}
    </SettingsDisclosure>
    <SettingsDisclosure title="字幕纠错" description="编辑替换规则，应用或撤销当前字幕的纠错" summary={rules !== printTerms(initial.corrections) || draft.correctionsEnabled !== initial.correctionsEnabled ? "未保存" : draft.correctionsEnabled ? "已启用" : "未启用"}>
    <label className="settings-row automatic-subtitle-row"><span>启用纠错规则</span><input className="toggle-switch" type="checkbox" role="switch" checked={draft.correctionsEnabled} disabled={busy} onChange={e => change({ correctionsEnabled: e.target.checked })} /></label>
    <div className="subtitle-preference-field"><SettingLabel htmlFor="subtitle-corrections" label="纠错规则" help={`此处是你明确启用的字面替换规则，与术语候选不同。不要将需要上下文判断的近音词填入此处。按文字精确替换，优先匹配较长原文，不连续套用规则。规则保存在独立层，可撤销，手工修正优先。保存后用于后续字幕任务；也可应用到当前音轨。修改原文会清除对应自动译文。`} /><textarea id="subtitle-corrections" className="input-text" rows={4} disabled={busy} value={rules} onChange={e => { setRules(e.target.value); setNotice(""); }} placeholder="误识别文字 => 正确文字" /></div>
    <div className="recognition-actions">
      <button type="button" className="btn btn-secondary" disabled={busy || dirty || !initial.correctionsEnabled} onClick={() => apply(false)}>应用到当前字幕</button>
      <button type="button" className="btn btn-ghost" disabled={busy} onClick={() => apply(true)}>撤销当前字幕规则</button>
    </div>
    </SettingsDisclosure>
    <div className="recognition-actions"><button type="button" className="btn btn-primary" disabled={busy || !dirty} onClick={() => void run(save)}>保存字幕偏好</button></div>
    {dirty && <p className="settings-field-hint">有未保存的修改，保存后可应用规则。</p>}
    {notice && <p role="status" className="settings-field-hint">{notice}</p>}
  </section>;
}
