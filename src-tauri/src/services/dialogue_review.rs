//! Same-model local rerecognition. Audio ownership, never text overlap, defines context.
#[cfg(test)]
use super::recognition::RecognitionService;
use crate::domain::{asr::*, AppError, AppResult, ErrorCode, SubtitleCueDto};
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Instant,
};

fn cancelled(cancel: &AtomicBool) -> AppResult<()> {
    if cancel.load(Ordering::SeqCst) {
        Err(AppError::new(ErrorCode::MediaDecodeFailed, "已取消识别"))
    } else {
        Ok(())
    }
}
fn compact(text: &str) -> String {
    super::speech_engine::clean_text(text)
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect()
}
#[cfg(test)]
fn voice_only(text: &str) -> bool {
    let text = compact(text);
    text.is_empty()
        || text
            .chars()
            .all(|c| "あぁうぅえぇおぉんンアァウゥエェオォはハふフへヘっッー".contains(c))
        || dialogue_only(&text).is_empty()
}
#[cfg(test)]
fn speech_family(model: &str) -> bool {
    model.starts_with("sensevoice")
}
pub const REVIEW_REVISION: u8 = 9;

/// Expanded decoder segments are not proof that speech happened in their padding.
/// Review only the original chunk's ownership, retaining the stored cue unchanged.
pub fn owned_cue(cue: &SubtitleCueDto, record: &AsrRecord) -> SubtitleCueDto {
    let mut owned = cue.clone();
    if let Some(initial) = record.attempts.first() {
        owned.start_ms = owned.start_ms.max(initial.start_ms);
        owned.end_ms = owned.end_ms.min(initial.end_ms);
    }
    owned
}
pub fn candidate(cue: &SubtitleCueDto, record: &AsrRecord) -> bool {
    candidate_with_repeat(cue, record, false)
}
fn candidate_with_repeat(cue: &SubtitleCueDto, record: &AsrRecord, repeated: bool) -> bool {
    let owned = owned_cue(cue, record);
    !record.human_confirmed
        && !record.confirmed_no_speech
        && record.dialogue_review.as_ref().is_none_or(|r| {
            r.revision < REVIEW_REVISION
                && r.decision != DialogueDecision::NonDialogue

        })
        && !compact(&cue.text).is_empty()
        && owned.end_ms > owned.start_ms
        && !dialogue_only(&cue.text).is_empty()
        && owned.end_ms - owned.start_ms <= 15_000
        && (repeated
            || owned.start_ms != cue.start_ms
            || owned.end_ms != cue.end_ms
            || compact(&cue.text).chars().count() <= 8
            || record.pending_audio_review
            || record.attempts.iter().any(|a| {
                a.signals.len() >= 2
                    || a.result
                        .segments
                        .iter()
                        .any(|s| s.no_speech_prob.is_some_and(|p| p >= 0.65))
            }))
}

/// Bounded, timed ASR evidence for translation disambiguation. Never use an
/// automatic translation as evidence, and never replace the stored original.
pub(crate) fn translation_audio_context(doc: &crate::db::subtitles::SubtitleDocument, id: usize) -> Option<String> {
    if doc.original_corrections.contains_key(&id) { return None; }
    let cue = doc.original.iter().find(|c| c.index == id)?;
    let record = doc.asr_records.get(&id)?;
    let review = record.dialogue_review.as_ref()?;
    if review.decision == DialogueDecision::NonDialogue { return None; }
    let owned = owned_cue(cue, record);
    let mut candidates: Vec<_> = review.observations.iter().filter(|o|
        o.context && valid_context_tokens(o) && !compact(&o.result.text).is_empty()
        && o.start_ms <= owned.start_ms && o.end_ms >= owned.end_ms
        && o.end_ms-o.start_ms <= 20_000 && o.result.text.chars().count() <= 300).collect();
    candidates.sort_by_key(|o| o.end_ms-o.start_ms);
    let mut seen = std::collections::BTreeSet::new();
    let values: Vec<_> = candidates.into_iter().filter(|o| seen.insert(compact(&o.result.text))).take(2)
        .map(|o| serde_json::json!({"model":o.model,"startMs":o.start_ms,"endMs":o.end_ms,"text":o.result.text})).collect();
    (!values.is_empty()).then(|| serde_json::to_string(&values).expect("ASR context serializes"))
}

#[cfg(test)]
pub fn cached_candidates(doc: &crate::db::subtitles::SubtitleDocument) -> Vec<usize> {
    cached_candidates_with_context(doc, None)
}
pub fn cached_candidates_with_context(doc: &crate::db::subtitles::SubtitleDocument, context_ms: Option<i64>) -> Vec<usize> {
    if !doc.source_key.starts_with("sensevoice:") || !doc.reference_subtitles.is_empty() {
        return vec![];
    }
    doc.original
        .iter()
        .enumerate()
        .filter(|(position, cue)| {
            let repeated = [position.checked_sub(1), Some(position + 1)]
                .into_iter()
                .flatten()
                .filter_map(|i| doc.original.get(i))
                .any(|other| {
                    (other.start_ms - cue.start_ms).abs() <= 30_000
                        && compact(&other.text) == compact(&cue.text)
                });
            !doc.original_corrections.contains_key(&cue.index)
                && !doc.translation_corrections.contains_key(&cue.index)
                && !doc.non_dialogue_cues.contains(&cue.index)
                && doc.asr_records.get(&cue.index).is_some_and(|r| {
                    let changed_context = context_ms.is_some_and(|ms| r.dialogue_review.as_ref().is_some_and(|review|
                        review.context_ms != ms && review.decision == DialogueDecision::Review));
                    if changed_context {
                        let mut next = r.clone();
                        next.dialogue_review = None;
                        candidate_with_repeat(cue, &next, repeated)
                    } else if repeated { candidate_with_repeat(cue, r, true) }
                    else { candidate(cue, r) }
                })
        })
        .map(|(_, cue)| cue.index)
        .collect()
}

/// Credible speech from either family vetoes hiding. A label or a probability
/// alone never decides; require two SenseVoice windows plus Whisper evidence.
#[cfg(test)]
fn context_is_after_cue(o: &DialogueObservation, owned_end: Option<i64>) -> bool {
    let Some(end) = owned_end else {
        return false;
    };
    let r = &o.result;
    if !o.context
        || r.tokens.is_empty()
        || r.tokens.len() != r.timestamps.len()
        || r.timestamps
            .iter()
            .any(|t| !t.is_finite() || *t < 0. || *t * 1000. > (o.end_ms - o.start_ms) as f64)
        || r.timestamps.windows(2).any(|t| t[0] > t[1])
        || compact(&r.tokens.concat().replace('▁', " ")) != compact(&r.text)
    {
        return false;
    }
    let speech: Vec<_> = r
        .tokens
        .iter()
        .zip(&r.timestamps)
        .filter(|(token, _)| !compact(token).is_empty())
        .collect();
    !speech.is_empty()
        && speech
            .iter()
            .all(|(_, t)| o.start_ms + (**t * 1000.).round() as i64 >= end + 100)
}
#[cfg(test)]
fn sense_supports_original(original: &str, observations: &[DialogueObservation]) -> bool {
    let original = compact(original);
    !original.is_empty() && observations.iter().any(|o|
        speech_family(&o.model) && !o.context && compact(&o.result.text).contains(&original)
        && (!voice_only(&o.result.text)
            || o.result.event.contains("Speech") && !o.result.event.contains("Breath")))
}
#[cfg(test)]
fn needs_independent_check(original: &str, observations: &[DialogueObservation]) -> bool {
    observations.iter().any(|o| o.model == "kotoba_whisper")
        && (!sense_supports_original(original, observations) || voice_only(original)
            || observations.iter().filter(|o| speech_family(&o.model) && o.context)
                .max_by_key(|o| o.end_ms-o.start_ms).is_some_and(|o| compact(&o.result.text).is_empty()))
        && !observations.iter().any(|o| matches!(o.model.as_str(), "whisper" | "faster_whisper")
            && !o.context && !compact(original).is_empty()
            && compact(&o.result.text).contains(&compact(original))
            && !o.result.segments.is_empty() && o.result.segments.iter().all(|s|
                s.no_speech_prob.is_some_and(|p| p.is_finite() && (0.0..0.5).contains(&p))
                    && s.avg_logprob.is_some_and(|p| p.is_finite() && p > -1.0)))
}
#[cfg(test)]
fn base_whisper_rejects(original: &str, observations: &[DialogueObservation]) -> bool {
    let original = compact(original);
    observations.iter().any(|o|
        matches!(o.model.as_str(), "whisper" | "faster_whisper") && !o.context
        && !original.is_empty() && !compact(&o.result.text).contains(&original)
        && !o.result.segments.is_empty() && o.result.segments.iter().all(|s|
            s.no_speech_prob.is_some_and(|p| p.is_finite() && (0.65..=1.0).contains(&p))))
}
// A fluent-looking distilled prediction is not corroborated by a base decoder
// stuck on syllabic loops. Require acoustic and cross-family evidence;
// ordinary repeated words, short answers and missing diagnostics do not qualify.
#[cfg(test)]
fn vocal_loop_evidence(original: &str, observations: &[DialogueObservation]) -> bool {
    let word = compact(original);
    if word.is_empty() || voice_only(original) { return false; }
    let Some(core) = observations.iter().find(|o| speech_family(&o.model) && !o.context) else { return false; };
    if !compact(&core.result.text).is_empty() { return false; }
    observations.iter().any(|o| base_family(&o.model) && !o.context
        && o.start_ms == core.start_ms && o.end_ms == core.end_ms
        && compact(&o.result.text).chars().count() >= 16
        && compact(&o.result.text).chars().all(|c| matches!(c, '\u{3041}'..='\u{3096}' | '\u{30a1}'..='\u{30fa}' | 'ー'))
        && !compact(&o.result.text).contains(&word)
        && !o.result.segments.is_empty() && o.result.segments.iter().all(|s|
            s.compression_ratio.is_some_and(|p| p.is_finite() && p > 2.4))
        && o.result.speech_activity.as_ref().is_some_and(|a|
            a.model == "silero-vad" && a.frames > 0
            && (0.0..0.2).contains(&a.mean_probability)
            && (a.mean_probability..=1.0).contains(&a.max_probability)))
}
#[cfg(test)]
fn vocal_loop_non_dialogue(original: &str, observations: &[DialogueObservation]) -> bool {
    if !vocal_loop_evidence(original, observations) { return false; }
    let core = observations.iter().find(|o| speech_family(&o.model) && !o.context).unwrap();
    // Recheck the same owned audio in two halves. This keeps a short quiet word
    // from being masked by a long window of environmental sound.
    let middle = core.start_ms + (core.end_ms-core.start_ms)/2;
    [(core.start_ms,middle),(middle,core.end_ms)].iter().all(|&(start,end)|
        observations.iter().any(|o| speech_family(&o.model) && !o.context
            && o.start_ms == start && o.end_ms == end && compact(&o.result.text).is_empty()))
}

#[cfg(test)]
fn needs_narrow_check(original: &str, observations: &[DialogueObservation]) -> bool {
    (!sense_supports_original(original, observations) || voice_only(original)) && base_whisper_rejects(original, observations)
}
/// Additional verification is bounded to a core window and a narrow context.
/// Keep failures distinct from an empty transcript and propagate cancellation.
fn observe(
    cue: &SubtitleCueDto, samples: &[f32], offset_ms: i64, model: &str, pad: i64,
    cancel: &AtomicBool, transcribe: impl FnOnce(&[f32]) -> AppResult<RecognitionEvidence>,
) -> AppResult<Option<DialogueObservation>> {
    observe_window(cue, samples, offset_ms, model, pad, pad, cancel, transcribe)
}
fn observe_window(
    cue: &SubtitleCueDto, samples: &[f32], offset_ms: i64, model: &str, before: i64, after: i64,
    cancel: &AtomicBool, transcribe: impl FnOnce(&[f32]) -> AppResult<RecognitionEvidence>,
) -> AppResult<Option<DialogueObservation>> {
    cancelled(cancel)?;
    let start_ms = (cue.start_ms - before).max(offset_ms);
    let end_ms = (cue.end_ms + after).min(offset_ms + samples.len() as i64 / 16);
    if start_ms > cue.start_ms || end_ms < cue.end_ms || end_ms <= start_ms || end_ms-start_ms > 20_000 {
        return Ok(None);
    }
    let started = Instant::now();
    let result = transcribe(&samples[((start_ms-offset_ms)*16) as usize..((end_ms-offset_ms)*16) as usize]);
    cancelled(cancel)?;
    Ok(result.ok().map(|result| DialogueObservation {
        model: model.into(), start_ms, end_ms, context: before > 0 || after > 0,
        elapsed_ms: started.elapsed().as_millis() as u64, result,
    }))
}
#[cfg(test)]
fn independently_non_dialogue(original: &str, observations: &[DialogueObservation]) -> bool {
    if !needs_narrow_check(original, observations) { return false; }
    let sv: Vec<_> = observations.iter().filter(|o| speech_family(&o.model)).collect();
    let Some(core) = sv.iter().find(|o| !o.context) else { return false; };
    let mut contexts: Vec<_> = sv.iter().filter(|o| o.context).collect();
    contexts.sort_by_key(|o| o.end_ms-o.start_ms);
    if contexts.len() != 2 { return false; }
    let narrow = contexts[0];
    let wide = contexts[1];
    if narrow.start_ms > core.start_ms || narrow.end_ms < core.end_ms
        || narrow.end_ms-narrow.start_ms >= wide.end_ms-wide.start_ms
        // The original rule is only for the extra 0.5 s window. A newer,
        // longer context must not accidentally inherit its looser text rule.
        || narrow.end_ms-narrow.start_ms > core.end_ms-core.start_ms+1000
        { return false; }
    let narrow_text = compact(&narrow.result.text);
    let low_activity = observations.iter().filter(|o| !o.context).any(|o|
        o.result.speech_activity.as_ref().is_some_and(|a|
            a.model == "silero-vad" && a.frames > 0
                && (0.0..0.2).contains(&a.max_probability)
                && (0.0..0.1).contains(&a.mean_probability)
                && a.mean_probability <= a.max_probability));
    // A short, different decoder fragment is not corroboration of the original
    // words. This exception requires independent acoustic activity AND the
    // base decoder's no-speech diagnosis AND an empty broad-context result.
    // A SenseVoice event label alone (including "Speech") is not decisive.
    if !voice_only(&narrow.result.text) && !(low_activity
        && narrow_text.chars().count() <= 3
        && !narrow_text.contains(&compact(original))
        && narrow_text != compact(&core.result.text)
        && compact(&wide.result.text).is_empty()) { return false; }
    // A tiny, uncorroborated core fragment may itself be a decoder artifact.
    // Never reject a lexical phrase or a fragment reproduced in another window.
    let fragment = compact(&core.result.text);
    if !voice_only(&core.result.text) && (fragment.chars().count() > 2
        || compact(&wide.result.text).contains(&fragment)) { return false; }
    if voice_only(&wide.result.text) { return true; }
    if context_is_after_cue(wide, Some(core.end_ms)) {
        // A weak response reproduced in both local windows remains possible,
        // even when a wider window only decodes the following sentence.
        return !(sense_supports_original(original, observations)
            && compact(&narrow.result.text).contains(&compact(original)));
    }
    // Wide context can include preceding speech. Require complete valid timing,
    // separation from the core, and the independent narrow window above.
    let r = &wide.result;
    !r.tokens.is_empty() && r.tokens.len() == r.timestamps.len()
        && compact(&r.tokens.concat().replace('▁', " ")) == compact(&r.text)
        && r.timestamps.iter().all(|t| t.is_finite() && *t >= 0.
            && *t*1000. <= (wide.end_ms-wide.start_ms) as f64)
        && r.timestamps.windows(2).all(|t| t[0] <= t[1])
        && r.tokens.iter().zip(&r.timestamps).filter(|(t,_)| !compact(t).is_empty())
            .all(|(_,t)| wide.start_ms+(*t*1000.).round() as i64 <= core.start_ms-750)
}

#[cfg(test)]
fn base_family(model: &str) -> bool {
    matches!(model, "whisper" | "faster_whisper")
}

// These are review evidence requirements, not decoder/VAD skip thresholds.
// A core prediction alone is especially unreliable on brief nonverbal sounds.
#[cfg(test)]
fn base_doubts_speech(o: &DialogueObservation) -> bool {
    base_family(&o.model) && !o.result.segments.is_empty()
        && o.result.segments.iter().all(|s|
            s.no_speech_prob.is_some_and(|p| p.is_finite() && (0.5..=1.0).contains(&p)))
}

#[cfg(test)]
fn context_check_windows(original: &str, observations: &[DialogueObservation]) -> Option<(i64, i64, i64)> {
    let original = compact(original);
    if !(1..=8).contains(&original.chars().count()) { return None; }
    let core = observations.iter().find(|o| speech_family(&o.model) && !o.context)?;
    if core.end_ms - core.start_ms > 4000 { return None; }
    let wide = observations.iter().filter(|o| speech_family(&o.model) && o.context)
        .max_by_key(|o| o.end_ms - o.start_ms)?;
    // Two genuinely different windows must fit; truncated edge context is not
    // proof of absence. Never substitute text from surrounding speech.
    let padding = (core.start_ms-wide.start_ms).min(wide.end_ms-core.end_ms);
    let preceding = preceding_context_only(&original, wide, core);
    // A single phoneme is also a possible real answer. Only collect further
    // evidence when the wide window is empty AND classified as nonverbal,
    // the core has no lexical phrase, and the independent decoder doubts
    // speech without reproducing that phoneme. Never infer this from length.
    if original.chars().count() == 1 && (!compact(&wide.result.text).is_empty()
        || !["Breath", "Laughter", "Cough", "Sneeze", "Cry"].iter().any(|e| wide.result.event.contains(e))
        || !voice_only(&core.result.text)
        || !observations.iter().any(|o| !o.context && base_family(&o.model)
            && !compact(&o.result.text).contains(&original)
            && !o.result.segments.is_empty() && o.result.segments.iter().all(|s|
                s.no_speech_prob.is_some_and(|p| p.is_finite() && (0.65..=1.0).contains(&p))))) {
        return None;
    }
    if padding < 2000 || (!compact(&wide.result.text).is_empty() && !preceding)
        || preceding && sense_supports_original(&original, observations)
        || !observations.iter().any(|o| !o.context && base_doubts_speech(o))
        || observations.iter().any(|o| !o.context && base_family(&o.model)
            && (o.start_ms != core.start_ms || o.end_ms != core.end_ms || !base_doubts_speech(o))) {
        return None;
    }
    // A segment-level no-speech score includes all input audio. When the wide
    // transcript is exclusively the preceding utterance, exclude that padding
    // from the independent check, retaining the entire core plus up to 2 s
    // after it. Do not move the subtitle boundary or silently crop its core.
    Some((padding / 2, if preceding { 0 } else { padding.min(2000) }, padding.min(2000)))
}

fn valid_context_tokens(o: &DialogueObservation) -> bool {
    let r = &o.result;
    !r.tokens.is_empty() && r.tokens.len() == r.timestamps.len()
        && compact(&r.tokens.concat().replace('▁', " ")) == compact(&r.text)
        && r.timestamps.iter().all(|t| t.is_finite() && *t >= 0.
            && *t * 1000. <= (o.end_ms-o.start_ms) as f64)
        && r.timestamps.windows(2).all(|t| t[0] <= t[1])
}

/// Neighbouring dialogue must not lend its confidence to this cue. A small
/// trailing anchor can cross a chunk boundary; require a clearly preceding
/// utterance and never discount the expected words, even a very short response.
#[cfg(test)]
fn preceding_context_only(original: &str, o: &DialogueObservation, core: &DialogueObservation) -> bool {
    let text = compact(&o.result.text);
    if text.is_empty() || text.contains(&compact(original)) || !valid_context_tokens(o) { return false; }
    let anchors: Vec<_> = o.result.tokens.iter().zip(&o.result.timestamps)
        .filter(|(token, _)| !compact(token).is_empty())
        .map(|(_, t)| o.start_ms + (*t * 1000.).round() as i64).collect();
    anchors.first().is_some_and(|t| *t <= core.start_ms-750)
        && anchors.last().is_some_and(|t| *t <= core.start_ms+250)
        // Without a later lexical anchor a multi-character final token could
        // still span the core. Punctuation at the file end is not an end time.
        && o.result.tokens.iter().rev().find(|t| !compact(t).is_empty())
            .is_some_and(|t| compact(t).chars().count() == 1)
}

/// Only reliable token anchors can show that a context transcript does not
/// corroborate the owned cue. The margin keeps boundary syllables conservative;
/// this never changes cue times or turns token anchors into invented word ends.
fn context_core_fragment(o: &DialogueObservation, core: &DialogueObservation) -> Option<String> {
    if compact(&o.result.text).is_empty() { return Some(String::new()); }
    if !valid_context_tokens(o) { return None; }
    let r = &o.result;
    let mut fragment = String::new();
    for (token, time) in r.tokens.iter().zip(&r.timestamps) {
        let at = o.start_ms + (*time * 1000.).round() as i64;
        if at >= core.start_ms-250 && at <= core.end_ms+250 {
            fragment.push_str(token);
        }
        // A multi-character token preceding the core has no trustworthy end.
        // Do not assume it ended before the cue merely from its start anchor.
        if at < core.start_ms-250 && compact(token).chars().count() > 1 {
            return None;
        }
    }
    Some(compact(&fragment))
}

#[cfg(test)]
fn context_non_dialogue(original: &str, observations: &[DialogueObservation]) -> bool {
    let Some((sense_pad, base_before, base_after)) = context_check_windows(original, observations) else { return false; };
    let core = observations.iter().find(|o| speech_family(&o.model) && !o.context).unwrap();
    // A complete word recovered in the local half-second context cannot be
    // discarded just because larger windows omit a quiet, brief response.
    let word = compact(original);
    if word.chars().count() >= 2 && observations.iter().any(|o| speech_family(&o.model) && o.context
        && o.start_ms <= core.start_ms && o.end_ms >= core.end_ms
        && o.end_ms-o.start_ms <= core.end_ms-core.start_ms+1000
        && compact(&o.result.text) == word && o.result.event.contains("Speech")
        && !o.result.event.contains("Breath") && valid_context_tokens(o)
        && context_core_fragment(o, core).as_deref() == Some(word.as_str())) { return false; }
    let Some(middle) = observations.iter().find(|o| speech_family(&o.model) && o.context
        && o.start_ms == core.start_ms-sense_pad && o.end_ms == core.end_ms+sense_pad) else { return false; };
    // One uncorroborated fragment is not a recovered full utterance. Single
    // characters need the stronger checks below. Full words, invalid timing
    // and ambiguous alignment veto this rule; no spelling is blacklisted.
    if !preceding_context_only(original, middle, core)
        && !context_core_fragment(middle, core).is_some_and(|s| s.chars().count() <= 1) { return false; }
    let Some(base) = observations.iter().find(|o| base_family(&o.model) && o.context
        && o.start_ms == core.start_ms-base_before && o.end_ms == core.end_ms+base_after) else { return false; };
    if compact(original).chars().count() == 1 {
        // Recovered timing must exclude the expected phoneme, even though a
        // different isolated fragment can be an unstable decoder artifact.
        // Independent reproduction in either base window vetoes exclusion.
        if !context_core_fragment(middle, core).is_some_and(|s| !s.contains(&compact(original)))
            || observations.iter().any(|o| base_family(&o.model)
                && compact(&o.result.text).contains(&compact(original))) { return false; }
    }
    base_doubts_speech(base) && base.result.speech_activity.as_ref().is_some_and(|a|
        a.model == "silero-vad" && a.frames > 0 && a.max_probability.is_finite()
            && (0.0..=1.0).contains(&a.max_probability)
            && (0.0..0.2).contains(&a.mean_probability)
            && a.mean_probability <= a.max_probability)
}

#[cfg(test)]
fn decide(
    original: &str,
    observations: Vec<DialogueObservation>,
    owned_end: Option<i64>,
) -> DialogueReview {
    let sv: Vec<_> = observations
        .iter()
        .filter(|o| speech_family(&o.model))
        .collect();
    let whisper = observations.iter().find(|o| !speech_family(&o.model));
    let Some(whisper) = whisper else {
        return unavailable("缺少 Whisper 对照，原文保留", observations);
    };
    if sv.len() < 2 || !sv.iter().any(|o| o.context) {
        return unavailable("缺少 SenseVoice 上下文对照，原文保留", observations);
    }
    let normalized = compact(original);
    let lexical_sv = sv
        .iter()
        .any(|o| !context_is_after_cue(o, owned_end) && !voice_only(&o.result.text));
    let empty_both_windows = sv
        .iter()
        .all(|o| compact(&o.result.text).is_empty() || context_is_after_cue(o, owned_end));
    let vocal_whisper = voice_only(&whisper.result.text);
    let uncertain_whisper = !whisper.result.segments.is_empty()
        && whisper.result.segments.iter().all(|s| {
            s.no_speech_prob.is_some_and(|p| p.is_finite() && p >= 0.65)
                && (s.avg_logprob.is_some_and(|p| p.is_finite() && p < -1.0)
                    || s.compression_ratio
                        .is_some_and(|p| p.is_finite() && p > 2.4)
                    || empty_both_windows)
        });
    let confident_whisper = !whisper.result.segments.is_empty()
        && whisper.result.segments.iter().all(|s| {
            s.no_speech_prob.is_some_and(|p| p.is_finite() && p < 0.5)
                && s.avg_logprob.is_some_and(|p| p.is_finite() && p > -1.0)
        });
    // No phrase blacklist. A plausible rendition (including a real sign-off)
    // overrides a high no-speech score. Short vocal responses remain ambiguous.
    let matched = !normalized.is_empty()
        && observations.iter().filter(|o| !o.context).any(|o| {
            compact(&o.result.text).contains(&normalized)
                && if speech_family(&o.model) {
                    !voice_only(&o.result.text)
                        || o.result.event.contains("Speech") && !o.result.event.contains("Breath")
                } else {
                    !uncertain_whisper && (!voice_only(&o.result.text) || confident_whisper)
                }
        });
    let nonverbal_event = sv.iter().any(|o| {
        ["Breath", "Laughter", "Cough", "Sneeze", "Cry"]
            .iter()
            .any(|label| o.result.event.contains(label))
    });
    let empty_context = sv.iter().any(|o| {
        o.context && (compact(&o.result.text).is_empty() || context_is_after_cue(o, owned_end))
    });
    let hidden = !lexical_sv
        && !matched
        && (uncertain_whisper
            || vocal_whisper && nonverbal_event
            || empty_both_windows && compact(&whisper.result.text).is_empty())
        && (empty_context || nonverbal_event);
    let (decision, note) = if vocal_loop_non_dialogue(original, &observations) {
        (DialogueDecision::NonDialogue,
            "独立解码器出现音节循环，语音活动较低，核心及分半复核均无台词；排除本次误识并保留原始记录")
    } else if context_non_dialogue(original, &observations) {
        (
            DialogueDecision::NonDialogue,
            "核心与上下文的独立语音诊断、多尺度音频复核及时间归属共同支持非台词，不生成字幕；原始识别与复核记录保留",
        )
    } else if independently_non_dialogue(original, &observations) {
        (
            DialogueDecision::NonDialogue,
            "独立模型诊断与核心、宽窄音频窗口共同支持非台词，不生成展示或翻译字幕；原始识别与复核记录保留",
        )
    } else if hidden {
        (
            DialogueDecision::NonDialogue,
            "局部及上下文复识别支持非台词，不生成台词或翻译；原始识别保留，可确认本句恢复",
        )
    } else if matched && !needs_independent_check(original, &observations) {
        (
            DialogueDecision::Retain,
            "复识别包含原台词，保留；不会因声音标签或单模型漏识删除",
        )
    } else {
        (
            DialogueDecision::Review,
            "模型对照存在差异，可能包含弱声对白、短应答或邻句尾音；原文保留待核对",
        )
    };
    DialogueReview {
        revision: REVIEW_REVISION,
        context_ms: 2500,
        decision,
        note: note.into(),
        observations,
    }
}
fn unavailable(note: &str, observations: Vec<DialogueObservation>) -> DialogueReview {
    DialogueReview {
        revision: REVIEW_REVISION,
        context_ms: 2500,
        decision: DialogueDecision::Unavailable,
        note: note.into(),
        observations,
    }
}

pub struct Reviewer {
    context_ms: i64,
}
impl Reviewer {
    pub fn new(context_ms: i64) -> Self { Self { context_ms: context_ms.clamp(0, 8000) } }
    pub fn review(
        &mut self, cue: &SubtitleCueDto, samples: &[f32], offset_ms: i64,
        primary: &str, cancel: &AtomicBool,
        mut transcribe: impl FnMut(&[f32]) -> AppResult<RecognitionEvidence>,
    ) -> AppResult<DialogueReview> {
        cancelled(cancel)?;
        let begin = cue.start_ms-offset_ms;
        let end = cue.end_ms-offset_ms;
        if begin < 0 || end <= begin || end > samples.len() as i64/16 || end-begin > 15_000 || samples.iter().any(|s| !s.is_finite()) {
            return Ok(unavailable("音频范围不足，原始识别保留待核对", vec![]));
        }
        let owned = &samples[begin as usize*16..end as usize*16];
        if owned.iter().all(|sample| *sample == 0.) {
            return Ok(DialogueReview {
                revision: REVIEW_REVISION, context_ms: self.context_ms,
                decision: DialogueDecision::NonDialogue,
                note: "核心音频为数字静音，不生成字幕；原始识别保留".into(),
                observations: vec![DialogueObservation {
                    model: primary.into(), start_ms: cue.start_ms, end_ms: cue.end_ms,
                    context: false, elapsed_ms: 0,
                    result: RecognitionEvidence { config: serde_json::json!({"inference":"skipped_exact_silence"}), ..Default::default() },
                }],
            });
        }
        let pad = self.context_ms.min((20_000-(end-begin))/2);
        let mut observations = vec![];
        // Reset text conditioning in the caller. Vary only the audio window of
        // this same loaded model; no second engine, download or majority vote.
        for padding in [0, pad] {
            match observe(cue, samples, offset_ms, primary, padding, cancel, &mut transcribe)? {
                Some(o) => observations.push(o),
                None => {
                    let mut result = unavailable("局部复识别失败，原始识别已保留", observations);
                    result.context_ms = self.context_ms;
                    return Ok(result);
                }
            }
        }
        let core = &observations[0];
        let context = &observations[1];
        let normal = compact(&cue.text);
        let timed_core = super::asr_quality::reliable_segments(&core.result, end-begin);
        let decoder_support = timed_core && core.result.segments.iter().all(|s|
            s.avg_logprob.is_some_and(|p| p.is_finite() && p > -1.0)
            && s.no_speech_prob.is_some_and(|p| p.is_finite() && p < 0.5)
            && s.compression_ratio.is_none_or(|r| r.is_finite() && r <= 2.4));
        let token_support = valid_context_tokens(core) && core.result.event.contains("Speech")
            && !core.result.event.contains("Breath");
        let context_owned = if super::asr_quality::reliable_segments(&context.result, context.end_ms-context.start_ms) {
            context.result.segments.iter().any(|s| {
                let start = context.start_ms+(s.start*1000.).round() as i64;
                let end = context.start_ms+(s.end*1000.).round() as i64;
                start >= cue.start_ms && end <= cue.end_ms && compact(&s.text) == normal
            })
        } else {
            context_core_fragment(context, core).as_deref() == Some(normal.as_str())
        };
        let supported = !normal.is_empty() && compact(&core.result.text) == normal
            && !super::asr_quality::repetitive(&core.result.text)
            && (decoder_support || token_support) && context_owned;
        Ok(DialogueReview {
            revision: REVIEW_REVISION, context_ms: self.context_ms,
            decision: if supported { DialogueDecision::Retain } else { DialogueDecision::Review },
            note: if supported { "同模型复识的核心时间范围支持原文，保留台词" }
                else { "复识缺少当前片段的可靠时间依据或结果有差异，可能含邻句延续；原始识别及复识结果保留待核对" }.into(),
            observations,
        })
    }
}

/// Reuse the first-pass retry evidence instead of decoding every fresh cue again.
pub fn finish_local_attempts(cue: &SubtitleCueDto, record: &mut AsrRecord, context_ms: i64) {
    if record.human_confirmed || record.confirmed_no_speech || record.dialogue_review.is_some() { return; }
    let observations = record.attempts.iter().take(3).map(|a| DialogueObservation {
        model: record.model.clone(), start_ms: a.start_ms, end_ms: a.end_ms,
        context: a.start_ms < cue.start_ms || a.end_ms > cue.end_ms,
        elapsed_ms: a.elapsed_ms, result: a.result.clone(),
    }).collect();
    let review = DialogueReview {
        revision: REVIEW_REVISION, context_ms,
        decision: if record.pending_audio_review { DialogueDecision::Review } else { DialogueDecision::Retain },
        note: record.note.clone(), observations,
    };
    apply(record, review);
}

#[cfg(test)]
fn review_audio(
    cue: &SubtitleCueDto,
    samples: &[f32],
    offset_ms: i64,
    primary: &str,
    peer_name: &str,
    cancel: &AtomicBool,
    mut transcribe: impl FnMut(&[f32], bool) -> AppResult<RecognitionEvidence>,
    context_ms: i64,
) -> AppResult<DialogueReview> {
    let mut observations = Vec::new();
    for (model, context, use_primary) in [
        (primary.to_owned(), false, true),
        (peer_name.to_owned(), false, false),
        (
            if speech_family(primary) {
                primary.to_owned()
            } else {
                peer_name.to_owned()
            },
            true,
            speech_family(primary),
        ),
    ] {
        cancelled(cancel)?;
        let pad = if context { context_ms.min((20_000-(cue.end_ms-cue.start_ms)).max(0)/2) } else { 0 };
        let begin_ms = (cue.start_ms - pad).max(offset_ms);
        let end_ms = (cue.end_ms + pad).min(offset_ms + samples.len() as i64 / 16);
        if begin_ms > cue.start_ms
            || end_ms < cue.end_ms
            || end_ms <= begin_ms
            || end_ms - begin_ms > 20_000
        {
            return Ok(unavailable("音频范围不足，保留原文待核对", observations));
        }
        let begin = ((begin_ms - offset_ms) * 16) as usize;
        let end = ((end_ms - offset_ms) * 16) as usize;
        let started = Instant::now();
        let result = transcribe(&samples[begin..end], use_primary);
        cancelled(cancel)?;
        match result {
            Ok(result) => observations.push(DialogueObservation {
                model,
                start_ms: begin_ms,
                end_ms,
                context,
                elapsed_ms: started.elapsed().as_millis() as u64,
                result,
            }),
            Err(_) => {
                // Disable a failing verifier for this job, never fail recognition or loop.
                return Ok(unavailable(
                    "局部跨模型复核失败，原始识别已保留",
                    observations,
                ));
            }
        }
    }
    Ok(decide(&cue.text, observations, Some(cue.end_ms)))
}

pub fn apply(record: &mut AsrRecord, review: DialogueReview) {
    if record.human_confirmed {
        return;
    }
    if matches!(
        review.decision,
        DialogueDecision::Review | DialogueDecision::NonDialogue
    ) {
        record.pending_audio_review = true;
    }
    record.note = review.note.clone();
    if let Some(previous) = record.dialogue_review.take() {
        record.dialogue_review_history.push(previous);
    }
    record.dialogue_review = Some(review);
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    fn decide(original: &str, observations: Vec<DialogueObservation>) -> DialogueReview {
        super::decide(original, observations, None)
    }
    fn evidence(text: &str, event: &str) -> RecognitionEvidence {
        RecognitionEvidence {
            text: text.into(),
            event: event.into(),
            ..Default::default()
        }
    }
    fn observation(model: &str, context: bool, text: &str, event: &str) -> DialogueObservation {
        DialogueObservation {
            model: model.into(),
            start_ms: 0,
            end_ms: 1000,
            context,
            elapsed_ms: 1,
            result: evidence(text, event),
        }
    }
    fn comparisons(
        speech: &str,
        whisper: &str,
        context: &str,
        event: &str,
    ) -> Vec<DialogueObservation> {
        vec![
            observation("sensevoice_fp16", false, speech, event),
            observation("faster_whisper", false, whisper, ""),
            observation("sensevoice_fp16", true, context, event),
        ]
    }
    #[test]
    fn nonlexical_loop_requires_two_empty_halves_and_keeps_real_dialogue() {
        let mut obs = comparisons("", &"んー、".repeat(30), "", "<|Speech|>");
        obs[1].result.segments = vec![SegmentEvidence {
            start: 0., end: 1., text: "んー、".repeat(30), avg_logprob: Some(-0.1),
            no_speech_prob: Some(0.4), compression_ratio: Some(10.),
        }];
        obs[1].result.speech_activity = Some(SpeechActivityEvidence {
            model: "silero-vad".into(), max_probability: 0.8, mean_probability: 0.1, frames: 30,
        });
        for (start,end) in [(0,500),(500,1000)] {
            let mut half = observation("sensevoice_fp16", false, "", "<|Speech|>");
            half.start_ms = start; half.end_ms = end; obs.push(half);
        }
        assert_eq!(decide("未知の単語", obs.clone()).decision, DialogueDecision::NonDialogue);
        let mut syllabic = obs.clone(); syllabic[1].result.text = "ふわっ、".repeat(30);
        assert!(vocal_loop_non_dialogue("未知の単語", &syllabic));
        let mut missing = obs.clone(); missing.pop();
        assert!(!vocal_loop_non_dialogue("未知の単語", &missing));
        for text in ["ごめん", "うん", "この名前は本当です"] {
            let mut spoken = obs.clone(); spoken[0].result.text = text.into();
            assert!(!vocal_loop_non_dialogue(text, &spoken));
        }
        let mut weak = obs.clone(); weak[3].result.text = "こんにちは".into();
        assert!(!vocal_loop_non_dialogue("未知の単語", &weak));
        let mut no_metric = obs.clone(); no_metric[1].result.speech_activity = None;
        assert!(!vocal_loop_non_dialogue("未知の単語", &no_metric));
        obs[1].result.text = "本当です".repeat(30);
        assert!(!vocal_loop_non_dialogue("未知の単語", &obs));
    }

    #[test]
    fn translation_audio_context_requires_timing_and_never_overrides_human_text() {
        let cue = SubtitleCueDto { index:7,start_ms:3000,end_ms:4000,text:"残り".into() };
        let mut doc = crate::db::subtitles::SubtitleDocument::new("v".into(),"sensevoice:test".into(),vec![cue],true);
        let mut r = record();
        let mut o = observation("sensevoice_fp16", true, "話の残り", "<|Speech|>");
        o.start_ms=2000; o.end_ms=5000;
        o.result.tokens=vec!["話".into(),"の".into(),"残".into(),"り".into()];
        o.result.timestamps=vec![0.1,0.3,1.2,1.5];
        r.dialogue_review=Some(unavailable("候选",vec![o]));
        doc.asr_records.insert(7,r);
        let raw=doc.original.clone();
        assert!(translation_audio_context(&doc,7).unwrap().contains("話の残り"));
        let input=crate::services::recognition::full_text_translation::review_input(&doc);
        assert!(input[0].audio_context.as_ref().unwrap().contains("話の残り"));
        doc.original_corrections.insert(7,"人工台词".into());
        assert!(translation_audio_context(&doc,7).is_none());
        doc.original_corrections.clear();
        doc.asr_records.get_mut(&7).unwrap().dialogue_review.as_mut().unwrap().observations[0].result.timestamps.clear();
        assert!(translation_audio_context(&doc,7).is_none());
        assert_eq!(doc.original,raw);
    }

    pub(crate) fn record() -> AsrRecord {
        AsrRecord {
            dialogue_review_history: vec![],
            model: "kotoba_whisper".into(),
            language: "ja".into(),
            attempts: vec![AsrAttempt {
                start_ms: 3000,
                end_ms: 4000,
                prompt: String::new(),
                elapsed_ms: 1,
                audio: AudioActivity::default(),
                result: evidence("ごめん", ""),
                signals: vec![],
                repetition_ratio: None,
            }],
            selected_attempt: 0,
            pending_audio_review: false,
            confirmed_no_speech: false,
            human_confirmed: false,
            note: String::new(),
            dialogue_review: None,
        }
    }
    fn cue() -> SubtitleCueDto {
        SubtitleCueDto {
            index: 7,
            start_ms: 3000,
            end_ms: 4000,
            text: "ごめん".into(),
        }
    }

    fn context_conflict(original: &str) -> Vec<DialogueObservation> {
        let mut obs = comparisons(original, original, "。", "<|Speech|>");
        obs[1].model = "kotoba_whisper".into();
        for o in &mut obs[..2] { o.start_ms=10000; o.end_ms=12000; }
        obs[2].start_ms=2000; obs[2].end_ms=20000;
        let mut base=observation("faster_whisper",false,"異なる出力","");
        base.start_ms=10000; base.end_ms=12000;
        base.result.segments.push(SegmentEvidence {no_speech_prob:Some(0.75),avg_logprob:Some(-1.2),..Default::default()});
        obs.push(base.clone());
        let mut middle=observation("sensevoice_fp16",true,"。","<|Speech|>");
        middle.start_ms=6000; middle.end_ms=16000; obs.push(middle);
        base.context=true; base.start_ms=8000; base.end_ms=14000;
        base.result.speech_activity=Some(SpeechActivityEvidence {
            model:"silero-vad".into(),frames:188,max_probability:0.8,mean_probability:0.1,
        });
        obs.push(base); obs
    }

    fn preceding_conflict(original: &str) -> Vec<DialogueObservation> {
        let mut obs=context_conflict(original);
        obs[0].result=evidence("ああ。","<|Speech|>");
        for (i, times) in [(2, vec![6.9,7.3,7.8,8.08]), (4,vec![2.9,3.3,3.8,4.08])] {
            obs[i].result=evidence("まだなの","<|Speech|>");
            obs[i].result.tokens="まだなの".chars().map(|c| c.to_string()).collect();
            obs[i].result.timestamps=times;
        }
        obs[5].start_ms=10000; // Entire core + future audio, no preceding dialogue.
        obs
    }

    #[test]
    fn t1513_39_confidence_belongs_to_owned_audio_not_the_previous_sentence() {
        for word in ["ごめん", "うん", "はい", "またね"] {
            let mut obs=preceding_conflict(word);
            obs[1].result.segments.push(SegmentEvidence {avg_logprob:Some(-0.1),no_speech_prob:Some(0.01),..Default::default()});
            assert_eq!(context_check_windows(word,&obs),Some((4000,0,2000)));
            assert_eq!(decide(word,obs.clone()).decision,DialogueDecision::NonDialogue);
            // A confident primary alone is insufficient, but independent
            // speech in the very same core protects an ordinary response.
            obs[3].result.text=word.into();
            obs[3].result.segments[0].no_speech_prob=Some(0.1);
            obs[3].result.segments[0].avg_logprob=Some(-0.3);
            assert_eq!(decide(word,obs).decision,DialogueDecision::Retain);
            let mut sense=preceding_conflict(word); sense[0].result=evidence(word,"<|Speech|>");
            assert_ne!(decide(word,sense).decision,DialogueDecision::NonDialogue);
        }
    }

    #[test]
    fn t1513_39_uncertain_boundary_or_real_context_speech_cannot_be_discarded() {
        let obs=preceding_conflict("ごめん");
        for i in [2,4] {
            for invalid in [vec![],vec![f64::NAN;4],vec![1.0,0.0,0.0,0.0],vec![2.9,3.3,3.8,99.0]] {
                let mut changed=obs.clone(); changed[i].result.timestamps=invalid;
                assert_ne!(decide("ごめん",changed).decision,DialogueDecision::NonDialogue);
            }
            let mut spoken=obs.clone();
            spoken[i].result=evidence("ごめん","<|Speech|>");
            spoken[i].result.tokens=vec!["ご".into(),"め".into(),"ん".into()];
            let offset=(10000-spoken[i].start_ms) as f64/1000.;
            spoken[i].result.timestamps=vec![offset+0.02,offset+0.08,offset+0.14];
            assert_ne!(decide("ごめん",spoken).decision,DialogueDecision::NonDialogue);
            let mut ambiguous=obs.clone();
            ambiguous[i].result.tokens=vec!["まだなの".into()];
            ambiguous[i].result.timestamps=vec![0.1];
            assert_ne!(decide("ごめん",ambiguous).decision,DialogueDecision::NonDialogue);
        }
        let mut previous_window=obs;
        previous_window[5].start_ms=8000; // A confident neighbour is not this evidence window.
        assert_ne!(decide("ごめん",previous_window).decision,DialogueDecision::NonDialogue);
    }

    #[test]
    fn t1513_39_forward_check_keeps_entire_core_and_cancels() {
        let c=SubtitleCueDto {index:7,start_ms:10000,end_ms:12000,text:"ごめん".into()};
        let flag=AtomicBool::new(false); let samples=vec![0.00001;16000*20];
        let o=observe_window(&c,&samples,0,"faster_whisper",0,2000,&flag,|s| {
            assert_eq!(s.len(),16000*4); Ok(evidence("",""))
        }).unwrap().unwrap();
        assert_eq!((o.start_ms,o.end_ms),(10000,14000));
        assert_eq!((c.start_ms,c.end_ms),(10000,12000));
        assert!(observe_window(&c,&samples,0,"faster_whisper",0,2000,&flag,|_| {
            flag.store(true,Ordering::SeqCst); Ok(evidence("",""))
        }).is_err());
        let mut r=record(); let mut old=unavailable("old",vec![]); old.revision=5; r.dialogue_review=Some(old);
        assert!(candidate(&cue(),&r));
    }

    #[test]
    fn t1513_38_cross_context_evidence_filters_sounds_not_words() {
        for text in ["うん", "ごめん", "はい", "またね"] {
            let obs=context_conflict(text);
            assert_eq!(decide(text,obs[..4].to_vec()).decision,DialogueDecision::Review);
            assert_eq!(decide(text,obs.clone()).decision,DialogueDecision::NonDialogue);
            // The identical word is kept when the independent core hears it.
            let mut spoken=obs.clone();
            spoken[3].result.text=text.into();
            spoken[3].result.segments[0].no_speech_prob=Some(0.1);
            spoken[3].result.segments[0].avg_logprob=Some(-0.3);
            assert_eq!(decide(text,spoken).decision,DialogueDecision::Retain);
            // A low-confidence base decoder cannot overrule timed speech,
            // including a weak response omitted by the longest context.
            let mut weak=obs;
            weak[4].result=evidence(text,"<|Speech|>");
            weak[4].result.tokens=vec![text.into()];
            weak[4].result.timestamps=vec![4.4];
            assert_ne!(decide(text,weak).decision,DialogueDecision::NonDialogue);
        }
        assert_ne!(decide("え",context_conflict("え")).decision,DialogueDecision::NonDialogue);
    }

    #[test]
    fn t1513_38_missing_or_conflicting_diagnostics_never_filter_weak_speech() {
        let obs=context_conflict("ごめん");
        for probability in [None,Some(0.1),Some(f64::NAN),Some(1.5)] {
            for index in [3,5] {
                let mut changed=obs.clone(); changed[index].result.segments[0].no_speech_prob=probability;
                assert_ne!(decide("ごめん",changed).decision,DialogueDecision::NonDialogue);
            }
        }
        for (mean,peak,frames) in [(0.3,0.8,188),(f64::NAN,0.8,188),(0.1,f64::NAN,188),(0.1,0.8,0),(0.1,0.01,188)] {
            let mut changed=obs.clone();
            changed[5].result.speech_activity=Some(SpeechActivityEvidence {model:"silero-vad".into(),mean_probability:mean,max_probability:peak,frames});
            assert_ne!(decide("ごめん",changed).decision,DialogueDecision::NonDialogue);
        }
        let mut changed=obs.clone(); changed[5].result.speech_activity=None;
        assert_ne!(decide("ごめん",changed).decision,DialogueDecision::NonDialogue);
        let mut changed=obs; changed[2].result.text="ごめんね".into();
        assert_ne!(decide("ごめん",changed).decision,DialogueDecision::NonDialogue);
    }

    #[test]
    fn t1513_41_single_phoneme_requires_nonverbal_context_and_no_recovered_answer() {
        let mut obs=context_conflict("う");
        obs[2].result.event="<|Breath|>".into();
        obs[4].result=evidence("ほ。","<|Speech|>");
        obs[4].result.tokens=vec!["ほ".into(),"。".into()];
        obs[4].result.timestamps=vec![4.2,9.8];
        assert_eq!(decide("う",obs.clone()).decision,DialogueDecision::NonDialogue);
        for i in [3,5] {
            let mut supported=obs.clone(); supported[i].result.text="う".into();
            assert_ne!(decide("う",supported).decision,DialogueDecision::NonDialogue);
            let mut missing=obs.clone(); missing[i].result.segments[0].no_speech_prob=None;
            assert_ne!(decide("う",missing).decision,DialogueDecision::NonDialogue);
        }
        let mut spoken=obs.clone(); spoken[4].result.tokens[0]="う".into(); spoken[4].result.text="う。".into();
        assert_ne!(decide("う",spoken).decision,DialogueDecision::NonDialogue);
        let mut no_event=obs.clone(); no_event[2].result.event="<|Speech|>".into();
        assert_ne!(decide("う",no_event).decision,DialogueDecision::NonDialogue);
        let mut invalid=obs.clone(); invalid[4].result.timestamps.clear();
        assert_ne!(decide("う",invalid).decision,DialogueDecision::NonDialogue);
        let mut phrase=obs; phrase[0].result.text="うれしい".into();
        assert_ne!(decide("う",phrase).decision,DialogueDecision::NonDialogue);
    }

    #[test]
    fn t1513_41_following_dialogue_does_not_confirm_a_nonverbal_core() {
        let mut obs=context_conflict("ごめん");
        obs[0].result=evidence("。","<|Speech|>");
        obs[4].start_ms=9500; obs[4].end_ms=12500;
        obs.truncate(5);
        obs[2].result=evidence("またね。","<|Speech|>");
        obs[2].result.tokens=vec!["ま".into(),"た".into(),"ね".into(),"。".into()];
        obs[2].result.timestamps=vec![12.0,12.2,12.4,17.9];
        assert_eq!(decide("ごめん",obs.clone()).decision,DialogueDecision::NonDialogue);
        for times in [vec![],vec![10.05,12.2,12.4,17.9],vec![f64::NAN;4]] {
            let mut ambiguous=obs.clone(); ambiguous[2].result.timestamps=times;
            assert_ne!(decide("ごめん",ambiguous).decision,DialogueDecision::NonDialogue);
        }
        let mut spoken=obs; spoken[0].result.text="ごめん".into();
        assert_ne!(decide("ごめん",spoken.clone()).decision,DialogueDecision::NonDialogue);
        spoken[0].result.text="うん".into(); spoken[4].result.text="うん".into();
        assert_ne!(decide("うん",spoken).decision,DialogueDecision::NonDialogue);
    }

    #[test]
    fn t1513_41_locally_recovered_words_survive_empty_larger_windows() {
        for word in ["はい", "うん", "ごめん"] {
            let mut obs=context_conflict(word);
            let mut narrow=observation("sensevoice_fp16",true,word,"<|Speech|>");
            narrow.start_ms=9500; narrow.end_ms=12500;
            narrow.result.tokens=word.chars().map(|c| c.to_string()).collect();
            narrow.result.timestamps=word.chars().enumerate().map(|(i,_)| 0.6+i as f64*0.1).collect();
            obs.push(narrow);
            assert_ne!(decide(word,obs).decision,DialogueDecision::NonDialogue);
        }
    }

    #[test]
    fn t1513_38_neighbouring_responses_do_not_confirm_owned_audio() {
        let mut obs=context_conflict("うん");
        obs[4].result=evidence("うんあうん。","<|Speech|>");
        obs[4].result.tokens=vec!["う".into(),"ん".into(),"あ".into(),"う".into(),"ん".into(),"。".into()];
        obs[4].result.timestamps=vec![0.1,0.3,4.5,8.1,8.3,9.8];
        assert_eq!(decide("うん",obs.clone()).decision,DialogueDecision::NonDialogue);
        for times in [vec![],vec![f64::NAN;6],vec![0.3,0.1,4.5,8.1,8.3,9.8],vec![0.1,0.3,4.5,8.1,8.3,12.0],vec![3.9,4.1,4.5,8.1,8.3,9.8]] {
            let mut changed=obs.clone(); changed[4].result.timestamps=times;
            assert_ne!(decide("うん",changed).decision,DialogueDecision::NonDialogue);
        }
        // Opaque multi-character tokens have no end anchor: do not guess.
        obs[4].result.tokens=vec!["うんあうん。".into()]; obs[4].result.timestamps=vec![0.1];
        assert_ne!(decide("うん",obs).decision,DialogueDecision::NonDialogue);
    }

    #[test]
    fn t1513_38_review_upgrade_saves_evidence_without_manual_hiding() {
        let mut c=cue(); c.start_ms=10000; c.end_ms=12000;
        let mut r=record(); r.attempts[0].start_ms=c.start_ms; r.attempts[0].end_ms=c.end_ms;
        let mut old=unavailable("previous",vec![]); old.revision=4; r.dialogue_review=Some(old);
        assert!(candidate(&c,&r));
        let mut obs=context_conflict("ごめん");
        let mut narrow=observation("sensevoice_fp16",true,"ごめん","<|Speech|>");
        narrow.start_ms=9500; narrow.end_ms=12500; obs.push(narrow);
        apply(&mut r,decide("ごめん",obs));
        let mut doc=crate::db::subtitles::SubtitleDocument::new("v".into(),"sensevoice:test".into(),vec![c],true);
        doc.asr_records.insert(7,r); doc.validate().unwrap();
        let mut loaded:crate::db::subtitles::SubtitleDocument=serde_json::from_str(&serde_json::to_string(&doc).unwrap()).unwrap();
        assert!(loaded.non_dialogue_cues.is_empty());
        assert_eq!(loaded.original,doc.original);
        assert_eq!(loaded.asr_records[&7].dialogue_review_history[0].revision,4);
        assert_eq!(loaded.asr_records[&7].dialogue_review.as_ref().unwrap().observations.len(),7);
        assert_eq!(loaded.cue_status(7),"nonDialogue"); assert!(!loaded.can_translate(7));
        assert!(cached_candidates(&loaded).is_empty());
        loaded.original_corrections.insert(7,"ごめん".into());
        loaded.asr_records.get_mut(&7).unwrap().human_confirmed=true;
        assert_eq!(loaded.rendered()[0].text,"ごめん"); assert!(loaded.can_translate(7));
    }

    #[test]
    fn long_empty_context_needs_independent_core_evidence() {
        let mut obs=comparisons("うん", "うん", "。", "<|Speech|>");
        obs[1].model="kotoba_whisper".into();
        obs[2].end_ms=18000;
        assert_eq!(decide("うん",obs.clone()).decision,DialogueDecision::Review);
        let mut independent=observation("faster_whisper",false,"うん","");
        independent.result.segments.push(SegmentEvidence {no_speech_prob:Some(0.1),avg_logprob:Some(-0.3),..Default::default()});
        obs.push(independent);
        assert_eq!(decide("うん",obs).decision,DialogueDecision::Retain);
    }
    #[test]
    fn bounded_audio_evidence_preserves_legacy_windows_without_rechecking_for_preferences() {
        let audio=vec![0.00001;40*16000];
        for duration in [2000,15000] {
            let cue=SubtitleCueDto {index:7,start_ms:10000,end_ms:10000+duration,text:"ごめん".into()};
            let mut lengths=vec![];
            let review=review_audio(&cue,&audio,0,"kotoba_whisper","sensevoice",&AtomicBool::new(false),|samples,_| {
                lengths.push(samples.len());Ok(evidence("ごめん","<|Speech|>"))
            },8000).unwrap();
            assert_eq!(lengths[0], duration as usize*16);
            assert_eq!(lengths[2],(duration+16000).min(20000) as usize*16);
            assert_eq!(review.observations[0].start_ms,10000);
        }
        let mut doc=crate::db::subtitles::SubtitleDocument::new("v".into(),"sensevoice:test".into(),vec![cue()],true);
        let mut r=record();let mut review=unavailable("fixture",vec![]);
        review.context_ms=2000;r.dialogue_review=Some(review);
        doc.asr_records.insert(7,r);
        assert!(cached_candidates(&doc).is_empty());
        // Historical expanded windows no longer trigger another review.
        assert!(cached_candidates(&doc).is_empty());
        doc.asr_records.get_mut(&7).unwrap().dialogue_review.as_mut().unwrap().context_ms=8000;
        assert!(cached_candidates(&doc).is_empty());
        doc.asr_records.get_mut(&7).unwrap().human_confirmed=true;
        assert!(cached_candidates(&doc).is_empty());
    }
    #[test]
    fn self_agreement_requires_independent_audio_corroboration() {
        for original in ["ごめん", "ありがとう", "ご視聴ありがとうございました", "はい"] {
            let mut obs = comparisons("。", original, "。", "<|Speech|>");
            obs[1].model = "kotoba_whisper".into();
            obs[1].result.segments.push(SegmentEvidence {
                avg_logprob: Some(-0.3), no_speech_prob: Some(0.1), ..Default::default()
            });
            assert!(needs_independent_check(original, &obs));
            assert_eq!(decide(original, obs.clone()).decision, DialogueDecision::Review);
            let mut independent = observation("faster_whisper", false, "unrelated output", "");
            independent.result.segments.push(SegmentEvidence {
                no_speech_prob: Some(0.9), avg_logprob: Some(-0.8), ..Default::default()
            });
            obs.push(independent);
            assert!(needs_narrow_check(original, &obs));
            assert_eq!(decide(original, obs.clone()).decision, DialogueDecision::Review);
            obs[2].end_ms = 6000; // Broad context, followed by separately decoded narrow context.
            obs.push(observation("sensevoice_fp16", true, "。", "<|Breath|>"));
            assert_eq!(decide(original, obs.clone()).decision, DialogueDecision::NonDialogue);
            // Quiet but lexical speech in the narrow window vetoes filtering.
            obs[4].result.text = "聞こえる？".into();
            assert_eq!(decide(original, obs.clone()).decision, DialogueDecision::Review);
            obs[4].result.text = "。".into();
            for probability in [None, Some(0.1), Some(f64::NAN), Some(2.0)] {
                obs[3].result.segments[0].no_speech_prob = probability;
                assert_eq!(decide(original, obs.clone()).decision, DialogueDecision::Review);
            }
            obs[0].result.text = original.into();
            obs[2].result.text = original.into();
            assert!(!needs_independent_check(original, &obs));
            assert_eq!(decide(original, obs).decision, DialogueDecision::Retain);
        }
    }
    #[test]
    fn low_activity_alone_never_removes_weak_speech() {
        let mut obs=comparisons("ま", "unrelated output", "。", "<|Breath|>");
        obs[1].result.segments.push(SegmentEvidence { no_speech_prob:Some(0.9), ..Default::default() });
        obs[2].end_ms=6000;
        obs.push(observation("sensevoice",true,"ね","<|Speech|>"));
        assert_eq!(decide("どうも",obs.clone()).decision,DialogueDecision::Review);
        obs[1].result.speech_activity=Some(SpeechActivityEvidence {
            model:"silero-vad".into(),max_probability:0.1,mean_probability:0.02,frames:60,
        });
        assert_eq!(decide("どうも",obs.clone()).decision,DialogueDecision::NonDialogue);
        for (peak,mean,frames) in [(0.8,0.02,60),(0.1,0.9,60),(f64::NAN,0.02,60),(0.1,0.02,0)] {
            let mut invalid=obs.clone();
            let activity=invalid[1].result.speech_activity.as_mut().unwrap();
            activity.max_probability=peak;activity.mean_probability=mean;activity.frames=frames;
            assert_eq!(decide("どうも",invalid).decision,DialogueDecision::Review);
        }
        obs[0].result.text="どうも".into();
        assert_eq!(decide("どうも",obs.clone()).decision,DialogueDecision::Retain);
        obs[0].result.text="ま".into();obs[3].result.text="聞こえますか".into();
        assert_eq!(decide("どうも",obs).decision,DialogueDecision::Review);
    }
    #[test]
    fn independent_check_keeps_unknown_context_and_unstable_timing() {
        let mut obs = comparisons("。", "other", "前の台詞", "<|Speech|>");
        obs[1].result.segments.push(SegmentEvidence { no_speech_prob: Some(0.9), ..Default::default() });
        for o in &mut obs[..2] { o.start_ms=3000; o.end_ms=4000; }
        obs[2].end_ms=6500;
        obs.push(observation("sensevoice_fp16", true, "。", "<|Speech|>"));
        obs[3].start_ms=2500;obs[3].end_ms=4500;
        assert_eq!(decide("ごめん", obs.clone()).decision, DialogueDecision::Review);
        obs[2].result.tokens=vec!["前の台詞".into()];obs[2].result.timestamps=vec![0.5];
        assert_eq!(decide("ごめん", obs.clone()).decision, DialogueDecision::NonDialogue);
        for t in [2.9, f64::NAN, -1.0, 99.0] {
            obs[2].result.timestamps[0]=t;
            assert_eq!(decide("ごめん", obs.clone()).decision, DialogueDecision::Review);
        }
    }
    #[test]
    fn extra_observation_cancels_and_never_turns_a_failure_into_silence() {
        let flag=AtomicBool::new(false);
        let audio=vec![0.00001;16000*8];
        assert!(observe(&cue(), &audio, 0, "fixture", 500, &flag, |_| {
            flag.store(true, Ordering::SeqCst); Ok(evidence("", ""))
        }).is_err());
        flag.store(false, Ordering::SeqCst);
        assert!(observe(&cue(), &audio, 0, "fixture", 500, &flag, |_| {
            Err(AppError::new(ErrorCode::MediaDecodeFailed, "test"))
        }).unwrap().is_none());
        let obs=observe(&cue(), &audio, 0, "fixture", 500, &flag, |s| {
            assert_eq!(s.len(), 32000); Ok(evidence("", ""))
        }).unwrap().unwrap();
        assert_eq!((obs.start_ms,obs.end_ms), (2500,4500));
    }
    #[test]
    fn independent_review_roundtrips_without_creating_manual_exclusions() {
        let mut doc=crate::db::subtitles::SubtitleDocument::new("v".into(),"sensevoice:test".into(),vec![cue()],true);
        let mut r=record();
        let mut old=unavailable("old", vec![]); old.revision=2;
        r.dialogue_review=Some(old);
        assert!(candidate(&cue(), &r));
        let mut obs=comparisons("。", "ごめん", "。", "<|Speech|>");
        obs[1].model="kotoba_whisper".into(); obs[2].end_ms=6000;
        let mut base=observation("faster_whisper",false,"other","");
        base.result.segments.push(SegmentEvidence {no_speech_prob:Some(0.9),..Default::default()});
        obs.push(base);obs.push(observation("sensevoice",true,"。","<|Breath|>"));
        apply(&mut r, decide("ごめん",obs));
        doc.asr_records.insert(7,r);
        doc.completed_chunks.insert(0);
        doc.validate().unwrap();
        let mut loaded:crate::db::subtitles::SubtitleDocument=serde_json::from_str(&serde_json::to_string(&doc).unwrap()).unwrap();
        assert!(loaded.non_dialogue_cues.is_empty());
        assert_eq!(loaded.original,doc.original);
        assert_eq!(loaded.completed_chunks,doc.completed_chunks);
        assert_eq!(loaded.cue_status(7),"nonDialogue");
        assert!(!loaded.can_translate(7));assert!(loaded.rendered().is_empty());
        assert!(cached_candidates(&loaded).is_empty());
        assert_eq!(loaded.asr_records[&7].dialogue_review_history.len(),1);
        loaded.original_corrections.insert(7,"ごめん".into());
        loaded.asr_records.get_mut(&7).unwrap().human_confirmed=true;
        assert_eq!(loaded.rendered()[0].text,"ごめん");
        assert!(loaded.can_translate(7));
    }
    #[test]
    fn ordinary_words_are_only_hidden_by_combined_audio_observations() {
        for text in ["ごめん", "またね", "ありがとう"] {
            let review = decide(text, comparisons("。", "ふぅ", "", "<|Breath|>"));
            assert_eq!(review.decision, DialogueDecision::NonDialogue);
            assert_eq!(review.observations.len(), 3);
            assert_eq!(
                decide(text, comparisons(text, "ふぅ", "", "<|Breath|>")).decision,
                DialogueDecision::Retain
            );
            assert_eq!(
                decide(text, comparisons("", text, "", "<|Breath|>")).decision,
                DialogueDecision::Retain
            );
        }
    }
    #[test]
    fn real_dialogue_signoffs_short_responses_and_context_tails_are_not_hidden() {
        for text in [
            "ほら",
            "ご視聴ありがとうございました",
            "ごちそうさまでした",
            "柔らかいね",
        ] {
            assert_eq!(
                decide(text, comparisons(text, "んんん", text, "<|Breath|>")).decision,
                DialogueDecision::Retain
            );
        }
        for text in ["うん", "はい", "ええ", "ああ", "ううん"] {
            assert_eq!(
                decide(text, comparisons(text, text, "", "<|Speech|>")).decision,
                DialogueDecision::Retain
            );
            assert_eq!(
                decide(text, comparisons("。", "ふぅ", "", "<|Breath|>")).decision,
                DialogueDecision::NonDialogue
            );
        }
        assert_eq!(
            decide(
                "ごめん",
                comparisons("", "ふぅ", "どうしてるの", "<|Breath|>")
            )
            .decision,
            DialogueDecision::Review
        );
    }
    #[test]
    fn missing_metrics_or_one_diagnostic_never_proves_a_hallucinated_phrase() {
        let mut obs = comparisons("", "ご視聴ありがとうございました", "", "<|Speech|>");
        assert_eq!(
            decide("ごめん", obs.clone()).decision,
            DialogueDecision::Review
        );
        obs[0].result.text = "ふぅ".into();
        obs[1].result.segments.push(SegmentEvidence {
            no_speech_prob: Some(0.99),
            ..Default::default()
        });
        assert_eq!(
            decide("ごめん", obs.clone()).decision,
            DialogueDecision::Review
        );
        obs[1].result.segments[0].avg_logprob = Some(-1.7);
        assert_eq!(
            decide("ごめん", obs.clone()).decision,
            DialogueDecision::NonDialogue
        );
        // The same unreliable Whisper phrase alone is not corroboration.
        obs[0].result.text = "ご視聴ありがとうございました".into();
        assert_eq!(
            decide("ご視聴ありがとうございました", obs).decision,
            DialogueDecision::Retain
        );
    }
    #[test]
    fn short_responses_require_audio_evidence_instead_of_blanket_protection() {
        for text in ["うん", "はい", "ごめん"] {
            let mut obs = comparisons("", text, "", "<|Speech|>");
            obs[1].result.segments.push(SegmentEvidence {
                no_speech_prob: Some(0.9),
                avg_logprob: Some(-0.8),
                ..Default::default()
            });
            assert_eq!(
                decide(text, obs.clone()).decision,
                DialogueDecision::NonDialogue
            );
            obs[1].result.segments[0].no_speech_prob = Some(0.1);
            assert_eq!(decide(text, obs).decision, DialogueDecision::Retain);
        }
        assert_eq!(
            decide("はい", comparisons("", "", "", "<|Speech|>")).decision,
            DialogueDecision::NonDialogue
        );
    }
    #[test]
    fn neighbouring_words_need_time_ownership_and_cannot_confirm_an_empty_core() {
        let mut obs = comparisons("", "", "ちょっと痛かった?", "<|Speech|>");
        // Context alone does not confirm that these words occurred in the cue.
        assert_eq!(
            super::decide("ちょっと痛かった?", obs.clone(), Some(1000)).decision,
            DialogueDecision::Review
        );
        obs[2].end_ms = 4000;
        obs[2].result.tokens = vec!["ちょっと痛かった?".into()];
        obs[2].result.timestamps = vec![1.5];
        assert_eq!(
            super::decide("ちょっと痛かった?", obs.clone(), Some(1000)).decision,
            DialogueDecision::NonDialogue
        );
        obs[2].result.timestamps = vec![0.5];
        assert_eq!(
            super::decide("ちょっと痛かった?", obs.clone(), Some(1000)).decision,
            DialogueDecision::Review
        );
        obs[2].result.timestamps = vec![f64::NAN];
        assert_eq!(
            super::decide("ちょっと痛かった?", obs, Some(1000)).decision,
            DialogueDecision::Review
        );
    }
    #[test]
    fn old_wide_segments_review_owned_audio_and_keep_previous_review_history() {
        let cue = SubtitleCueDto {
            index: 7,
            start_ms: 409000,
            end_ms: 421400,
            text: "ちょっと痛かった?".into(),
        };
        let mut r = record();
        r.attempts[0].start_ms = 412000;
        r.attempts[0].end_ms = 420000;
        let mut old = unavailable("old", vec![]);
        old.revision = 0;
        r.dialogue_review = Some(old);
        assert!(candidate(&cue, &r));
        let owned = owned_cue(&cue, &r);
        assert_eq!((owned.start_ms, owned.end_ms), (412000, 420000));
        apply(
            &mut r,
            decide(&cue.text, comparisons("", "", "", "<|Speech|>")),
        );
        assert!(!candidate(&cue, &r));
        assert_eq!(r.dialogue_review_history.len(), 1);
        let reopened: AsrRecord =
            serde_json::from_value(serde_json::to_value(&r).unwrap()).unwrap();
        assert_eq!(reopened.dialogue_review_history[0].note, "old");
        assert_eq!(cue.start_ms, 409000); // Only the review window changed, never the stored timeline.
    }
    #[test]
    fn repeated_long_lines_are_candidates_but_are_not_textually_deleted() {
        let mut doc = crate::db::subtitles::SubtitleDocument::new(
            "v".into(),
            "sensevoice:test".into(),
            vec![],
            true,
        );
        for i in 0..2 {
            let c = SubtitleCueDto {
                index: i,
                start_ms: i as i64 * 10000,
                end_ms: i as i64 * 10000 + 4000,
                text: "今日はとてもいい天気ですね".into(),
            };
            let mut r = record();
            r.attempts[0].start_ms = c.start_ms;
            r.attempts[0].end_ms = c.end_ms;
            doc.original.push(c);
            doc.asr_records.insert(i, r);
        }
        assert_eq!(cached_candidates(&doc), vec![0, 1]);
        assert_eq!(doc.rendered().len(), 2);
        let text = &doc.original[0].text;
        assert_eq!(
            decide(text, comparisons(text, text, text, "<|Speech|>")).decision,
            DialogueDecision::Retain
        );
    }
    #[test]
    fn missing_peer_keeps_speech_translatable_and_completed_reviews_do_not_loop() {
        let mut doc = crate::db::subtitles::SubtitleDocument::new(
            "v".into(),
            "sensevoice:test".into(),
            vec![cue()],
            true,
        );
        doc.asr_records.insert(7, record());
        assert_eq!(cached_candidates(&doc), vec![7]);
        apply(
            doc.asr_records.get_mut(&7).unwrap(),
            unavailable("no peer", vec![]),
        );
        assert!(cached_candidates(&doc).is_empty());
        assert!(doc.can_translate(7));
        let mut human = record();
        human.human_confirmed = true;
        apply(
            &mut human,
            decide("ごめん", comparisons("", "ふぅ", "", "<|Breath|>")),
        );
        assert!(human.dialogue_review.is_none());
        for protection in 0..4 {
            doc.asr_records.insert(7, record());
            doc.original_corrections.clear();
            doc.translation_corrections.clear();
            doc.non_dialogue_cues.clear();
            if protection == 0 {
                doc.original_corrections.insert(7, "ごめん".into());
            }
            if protection == 1 {
                doc.translation_corrections.insert(7, "对不起".into());
            }
            if protection == 2 {
                doc.non_dialogue_cues.insert(7);
            }
            if protection == 3 {
                doc.source_key = "external:test".into();
            }
            assert!(cached_candidates(&doc).is_empty());
        }
    }
    #[test]
    fn bounded_audio_review_preserves_windows_and_stops_on_cancel_or_error() {
        let audio = vec![0.00001; 8 * 16000]; // Weak synthetic signal is not filtered by volume.
        for (primary, peer) in [
            ("kotoba_whisper", "sensevoice"),
            ("sensevoice_fp16", "faster_whisper"),
        ] {
            let flag = AtomicBool::new(false);
            let mut calls = vec![];
            let review = review_audio(
                &cue(),
                &audio,
                0,
                primary,
                peer,
                &flag,
                |samples, primary_call| {
                    calls.push((samples.len(), primary_call));
                    Ok(evidence("ごめん", "<|Speech|>"))
                },
                2500,
            )
            .unwrap();
            assert_eq!(review.decision, DialogueDecision::Retain);
            assert_eq!(
                calls,
                vec![
                    (16000, true),
                    (16000, false),
                    (96000, speech_family(primary))
                ]
            );
            assert_eq!(
                (
                    review.observations[2].start_ms,
                    review.observations[2].end_ms
                ),
                (500, 6500)
            );
        }
        let flag = AtomicBool::new(false);
        let mut calls = 0;
        assert!(
            review_audio(&cue(), &audio, 0, "whisper", "sensevoice", &flag, |_, _| {
                calls += 1;
                flag.store(true, Ordering::SeqCst);
                Ok(evidence("", ""))
            }, 2500)
            .is_err()
        );
        assert_eq!(calls, 1);
        flag.store(false, Ordering::SeqCst);
        calls = 0;
        let review = review_audio(&cue(), &audio, 0, "whisper", "sensevoice", &flag, |_, _| {
            calls += 1;
            Err(AppError::new(ErrorCode::MediaDecodeFailed, "fixture error"))
        }, 2500)
        .unwrap();
        assert_eq!(calls, 1);
        assert_eq!(review.decision, DialogueDecision::Unavailable);
        let review = review_audio(
            &cue(),
            &audio,
            5000,
            "whisper",
            "sensevoice",
            &flag,
            |_, _| panic!("out of range"),
            2500,
        )
        .unwrap();
        assert_eq!(review.decision, DialogueDecision::Unavailable);
    }
    #[test]
    fn local_rerecognition_uses_only_supplied_model_and_preserves_normal_responses() {
        for word in ["うん", "ごめん", "ご視聴ありがとうございました"] {
            let mut reviewer = Reviewer::new(8000);
            let cue = SubtitleCueDto { index: 7, start_ms: 8000, end_ms: 10000, text: word.into() };
            let mut calls = vec![];
            let review = reviewer.review(&cue, &vec![0.00001; 20*16000], 0,
                "kotoba_whisper", &AtomicBool::new(false), |samples| {
                    calls.push(samples.len());
                    let mut result = evidence(word, "");
                    result.segments.push(SegmentEvidence { start: if calls.len()==1 {0.} else {8.},
                        end: if calls.len()==1 {2.} else {10.}, text: word.into(),
                        avg_logprob: Some(-0.2), no_speech_prob: Some(0.1), ..Default::default() });
                    Ok(result)
                }).unwrap();
            assert_eq!(calls, vec![2*16000, 18*16000]);
            assert_eq!(review.decision, DialogueDecision::Retain);
            assert!(review.observations.iter().all(|o| o.model=="kotoba_whisper"));
        }
    }
    #[test]
    fn local_rerecognition_keeps_history_and_retries_changed_context_once() {
        let mut doc=crate::db::subtitles::SubtitleDocument::new("v".into(),"sensevoice:test".into(),vec![cue()],true);
        let mut r=record();
        r.pending_audio_review=true;
        let raw=r.attempts[0].result.text.clone();
        finish_local_attempts(&cue(),&mut r,2000);
        doc.asr_records.insert(7,r);
        assert!(cached_candidates_with_context(&doc,Some(2000)).is_empty());
        assert_eq!(cached_candidates_with_context(&doc,Some(8000)),vec![7]);
        let r=doc.asr_records.get_mut(&7).unwrap();
        let mut next=r.dialogue_review.clone().unwrap(); next.context_ms=8000;
        apply(r,next);
        assert_eq!(r.dialogue_review_history.len(),1);
        assert_eq!(r.attempts[0].result.text,raw);
        doc.validate().unwrap();
        assert!(cached_candidates_with_context(&doc,Some(8000)).is_empty());
        doc.translation_corrections.insert(7,"人工台词".into());
        assert!(cached_candidates_with_context(&doc,Some(2000)).is_empty());
        assert_eq!(doc.translation_text(7),Some("人工台词"));
    }
    #[test]
    fn local_rerecognition_context_agreement_cannot_validate_a_neighbour() {
        let cue = SubtitleCueDto { index: 7, start_ms: 8000, end_ms: 10000, text: "うん".into() };
        let mut reviewer = Reviewer::new(8000);
        let mut calls = 0;
        let review = reviewer.review(&cue, &vec![0.01;20*16000],0,"kotoba_whisper",&AtomicBool::new(false),|_| {
            calls+=1;
            let mut result=evidence("うん","");
            result.segments.push(SegmentEvidence {start:0.,end:2.,text:"うん".into(),avg_logprob:Some(-0.2),no_speech_prob:Some(0.01),..Default::default()});
            Ok(result)
        }).unwrap();
        assert_eq!(calls,2);
        assert_eq!(review.decision,DialogueDecision::Review);
        let flag=AtomicBool::new(false);
        let silence=reviewer.review(&cue,&vec![0.;20*16000],0,"kotoba_whisper",&flag,|_|panic!("digital silence")).unwrap();
        assert_eq!(silence.decision,DialogueDecision::NonDialogue);
        flag.store(true,Ordering::SeqCst);
        assert!(reviewer.review(&cue,&vec![0.01;20*16000],0,"kotoba_whisper",&flag,|_|panic!("cancelled")).is_err());
    }

    #[test]
    #[ignore = "requires an authorized private subtitle snapshot and complete audio-review results"]
    fn local_review_document_roundtrip() {
        use crate::db::subtitles::SubtitleDocument;
        #[derive(serde::Deserialize)]
        struct ResultRow { id: usize, review: DialogueReview }
        let directory = std::path::PathBuf::from(std::env::var("ASMR_REVIEW_DOCUMENT_DIR").unwrap());
        let before: SubtitleDocument = serde_json::from_slice(
            &std::fs::read(directory.join("before.json")).unwrap(),
        ).unwrap();
        before.validate().unwrap();
        assert!(before.source_key.starts_with("sensevoice:"));
        assert!(before.reference_subtitles.is_empty());
        let results: Vec<ResultRow> = serde_json::from_slice(
            &std::fs::read(directory.join("cases.review.json")).unwrap(),
        ).unwrap();
        let ids: std::collections::BTreeSet<_> = results.iter().map(|r| r.id).collect();
        assert_eq!(ids.len(), results.len(), "duplicate review ID");
        let all_ids: std::collections::BTreeSet<_> = before.original.iter().map(|c| c.index).collect();
        let expected_path = directory.join("expected-ids.json");
        let expected_ids = if expected_path.is_file() {
            serde_json::from_slice(&std::fs::read(expected_path).unwrap()).unwrap()
        } else { all_ids.clone() };
        assert!(ids.is_subset(&all_ids), "unknown review ID");
        assert_eq!(ids, expected_ids, "incomplete review");
        let mut after = before.clone();
        let mut applied = 0;
        for result in results {
            assert_eq!(result.review.revision, REVIEW_REVISION);
            assert_ne!(result.review.decision, DialogueDecision::Unavailable);
            let cue = before.original.iter().find(|c| c.index == result.id).unwrap();
            let owned = owned_cue(cue, &before.asr_records[&result.id]);
            assert_eq!(super::decide(&cue.text, result.review.observations.clone(), Some(owned.end_ms)).decision,
                result.review.decision, "saved observations disagree with current rules for {}", result.id);
            let protected = before.original_corrections.contains_key(&result.id)
                || before.translation_corrections.contains_key(&result.id)
                || before.non_dialogue_cues.contains(&result.id)
                || before.trusted_reference(result.id).is_some();
            let record = after.asr_records.get_mut(&result.id).expect("original ASR record");
            // Match incremental recognition: never overturn a previous confirmed
            // non-dialogue result or a human edit during an automatic cache review.
            if protected || record.human_confirmed || record.confirmed_no_speech
                || record.dialogue_review.as_ref().is_some_and(|r| r.decision == DialogueDecision::NonDialogue) {
                continue;
            }
            apply(record, result.review);
            applied += 1;
        }
        after.validate().unwrap();
        assert_eq!(before.original, after.original);
        let before_json = serde_json::to_value(&before).unwrap();
        let after_json = serde_json::to_value(&after).unwrap();
        for (key, value) in before_json.as_object().unwrap() {
            if key != "asrRecords" { assert_eq!(value, &after_json[key], "changed {key}"); }
        }
        for (id, record) in &before.asr_records {
            let key = id.to_string();
            for field in ["model", "language", "attempts", "selectedAttempt", "humanConfirmed", "confirmedNoSpeech"] {
                assert_eq!(before_json["asrRecords"][&key][field], after_json["asrRecords"][&key][field]);
            }
            let updated = &after.asr_records[id];
            if updated.dialogue_review_history.len() > record.dialogue_review_history.len() {
                assert_eq!(serde_json::to_value(updated.dialogue_review_history.last()).unwrap(),
                    serde_json::to_value(&record.dialogue_review).unwrap());
            }
            if after.cue_status(*id) == "nonDialogue" {
                assert!(!after.can_translate(*id));
                assert!(!after.rendered().iter().any(|c| c.index == *id));
            }
        }
        // Exercise the normal persistence/validation path only in a temporary DB.
        let root = tempfile::tempdir().unwrap();
        let db = crate::db::Database::open_and_migrate(&root.path().join("data"), &root.path().join("cache")).unwrap();
        db.with_conn(|conn| {
            conn.execute_batch("INSERT INTO works(id,title,created_at,updated_at) VALUES('w','fixture','now','now'); INSERT INTO tracks(id,work_id,title,sort_key,created_at) VALUES('t','w','fixture','1','now');")?;
            conn.execute("INSERT INTO track_versions(id,track_id,version_label,created_at) VALUES(?1,'t','original','now')", [&after.version_id])?;
            Ok(())
        }).unwrap();
        db.save_subtitle_document(&after).unwrap();
        let reopened = db.subtitle_document(&after.version_id, &after.source_key).unwrap().unwrap();
        assert_eq!(serde_json::to_value(&after).unwrap(), serde_json::to_value(&reopened).unwrap());
        assert_eq!(after.rendered(), reopened.rendered());
        std::fs::write(directory.join("after.json"), serde_json::to_vec(&after).unwrap()).unwrap();
        println!("Reviewed {} records, applied {applied}; rendered {} -> {}", ids.len(), before.rendered().len(), after.rendered().len());
    }

    #[test]
    #[ignore = "requires authorized local mono 16kHz f32 PCM cases and installed recognition models"]
    fn local_audio_review_smoke() {
        #[derive(serde::Deserialize)]
        struct Case {
            cue: SubtitleCueDto,
            pcm: String,
            offset_ms: i64,
            #[serde(default)]
            expected: Option<DialogueDecision>,
        }
        let model_root =
            std::path::PathBuf::from(std::env::var("ASMR_REVIEW_MODEL_ROOT").expect("model root"));
        let case_path = std::path::PathBuf::from(
            std::env::var("ASMR_REVIEW_CASES").expect("private case manifest"),
        );
        let cases: Vec<Case> = serde_json::from_slice(&std::fs::read(&case_path).unwrap()).unwrap();
        let root = tempfile::tempdir().unwrap();
        let mut db = crate::db::Database::open_and_migrate(
            &root.path().join("data"),
            &root.path().join("cache"),
        )
        .unwrap();
        // The connection stays in the temporary directory; only model lookup uses the authorized root.
        db.data_dir = model_root.clone();
        let service = RecognitionService::new(db, model_root, "test");
        let primary_name = std::env::var("ASMR_REVIEW_PRIMARY").unwrap_or("faster_whisper".into());
        service.require_engine_for(&primary_name).unwrap();
        let cancel = AtomicBool::new(false);
        let mut fast = if primary_name.contains("whisper") {
            Some(super::super::faster_whisper::FasterWhisper::new(
                &service.ct2_directory(&primary_name).unwrap(),
                &service.sensevoice_fp16_directory(), "ja", "", &cancel).unwrap())
        } else { None };
        let mut gpu = if primary_name == "sensevoice_fp16" {
            Some(super::super::sensevoice_gpu::GpuRecognizer::for_local_test(
                std::path::Path::new(&std::env::var("ASMR_REVIEW_WORKER").expect("headless GPU worker")),
                &service.sensevoice_fp16_directory(), "ja", &cancel).unwrap())
        } else { None };
        let padding = std::env::var("ASMR_REVIEW_CONTEXT_MS").ok().and_then(|s| s.parse().ok()).unwrap_or(2000);
        let mut reviewer = Reviewer::new(padding);
        let mut results = vec![];
        let mut unexpected = vec![];
        for case in cases {
            let raw = std::fs::read(&case.pcm).unwrap();
            assert_eq!(raw.len() % 4, 0);
            let samples: Vec<_> = raw
                .chunks_exact(4)
                .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
                .collect();
            let result = reviewer
                .review(
                    &case.cue,
                    &samples,
                    case.offset_ms,
                    &primary_name,
                    &cancel,
                    |audio| if let Some(engine) = fast.as_mut() { engine.transcribe_comparison(audio, &cancel, true) }
                        else { gpu.as_mut().expect("explicit test engine").transcribe(audio, &cancel) },
                )
                .unwrap();
            assert_ne!(result.decision, DialogueDecision::Unavailable);
            if case.expected.as_ref().is_some_and(|expected| *expected != result.decision) {
                unexpected.push(case.cue.index);
            }
            println!(
                "case {}: {:?}, {} ms",
                case.cue.index,
                result.decision,
                result
                    .observations
                    .iter()
                    .map(|o| o.elapsed_ms)
                    .sum::<u64>()
            );
            results.push(serde_json::json!({"id":case.cue.index,"review":result}));
        }
        std::fs::write(
            case_path.with_extension("review.json"),
            serde_json::to_vec_pretty(&results).unwrap(),
        )
        .unwrap();
        assert!(unexpected.is_empty(), "unexpected audio decisions: {unexpected:?}");
    }
}
