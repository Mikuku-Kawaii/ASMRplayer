//! Local, bounded, cross-family audio review. Never rewrite words or invent timing.
use super::{recognition::RecognitionService, speech_engine::SenseVoice};
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
fn voice_only(text: &str) -> bool {
    let text = compact(text);
    text.is_empty()
        || text
            .chars()
            .all(|c| "あぁうぅえぇおぉんンアァウゥエェオォはハふフへヘっッー".contains(c))
        || dialogue_only(&text).is_empty()
}
fn speech_family(model: &str) -> bool {
    model.starts_with("sensevoice")
}
pub const REVIEW_REVISION: u8 = 4;

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
    candidate_with_context(cue, record, repeated, None)
}
fn candidate_with_context(cue: &SubtitleCueDto, record: &AsrRecord, repeated: bool, context_ms: Option<i64>) -> bool {
    let owned = owned_cue(cue, record);
    !record.human_confirmed
        && !record.confirmed_no_speech
        && record.dialogue_review.as_ref().is_none_or(|r| {
            (r.revision < REVIEW_REVISION || context_ms.is_some_and(|ms| r.context_ms != ms))
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

pub fn cached_candidates(doc: &crate::db::subtitles::SubtitleDocument) -> Vec<usize> {
    cached_candidates_for_context(doc, None)
}
pub fn cached_candidates_for_context(doc: &crate::db::subtitles::SubtitleDocument, context_ms: Option<i64>) -> Vec<usize> {
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
                    if context_ms.is_some() { candidate_with_context(cue, r, repeated, context_ms) }
                    else if repeated { candidate_with_repeat(cue, r, true) }
                    else { candidate(cue, r) }
                })
        })
        .map(|(_, cue)| cue.index)
        .collect()
}

/// Credible speech from either family vetoes hiding. A label or a probability
/// alone never decides; require two SenseVoice windows plus Whisper evidence.
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
fn sense_supports_original(original: &str, observations: &[DialogueObservation]) -> bool {
    let original = compact(original);
    !original.is_empty() && observations.iter().any(|o|
        speech_family(&o.model) && !o.context && compact(&o.result.text).contains(&original)
        && (!voice_only(&o.result.text)
            || o.result.event.contains("Speech") && !o.result.event.contains("Breath")))
}
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
fn base_whisper_rejects(original: &str, observations: &[DialogueObservation]) -> bool {
    let original = compact(original);
    observations.iter().any(|o|
        matches!(o.model.as_str(), "whisper" | "faster_whisper") && !o.context
        && !original.is_empty() && !compact(&o.result.text).contains(&original)
        && !o.result.segments.is_empty() && o.result.segments.iter().all(|s|
            s.no_speech_prob.is_some_and(|p| p.is_finite() && (0.65..=1.0).contains(&p))))
}
fn needs_narrow_check(original: &str, observations: &[DialogueObservation]) -> bool {
    (!sense_supports_original(original, observations) || voice_only(original)) && base_whisper_rejects(original, observations)
}
/// Additional verification is bounded to a core window and a narrow context.
/// Keep failures distinct from an empty transcript and propagate cancellation.
fn observe(
    cue: &SubtitleCueDto, samples: &[f32], offset_ms: i64, model: &str, pad: i64,
    cancel: &AtomicBool, transcribe: impl FnOnce(&[f32]) -> AppResult<RecognitionEvidence>,
) -> AppResult<Option<DialogueObservation>> {
    cancelled(cancel)?;
    let start_ms = (cue.start_ms - pad).max(offset_ms);
    let end_ms = (cue.end_ms + pad).min(offset_ms + samples.len() as i64 / 16);
    if start_ms > cue.start_ms || end_ms < cue.end_ms || end_ms <= start_ms || end_ms-start_ms > 20_000 {
        return Ok(None);
    }
    let started = Instant::now();
    let result = transcribe(&samples[((start_ms-offset_ms)*16) as usize..((end_ms-offset_ms)*16) as usize]);
    cancelled(cancel)?;
    Ok(result.ok().map(|result| DialogueObservation {
        model: model.into(), start_ms, end_ms, context: pad > 0,
        elapsed_ms: started.elapsed().as_millis() as u64, result,
    }))
}
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
    let (decision, note) = if independently_non_dialogue(original, &observations) {
        (
            DialogueDecision::NonDialogue,
            "独立模型诊断与核心、宽窄音频窗口共同支持非台词，不生成展示或翻译字幕；原始识别与复核记录保留",
        )
    } else if hidden {
        (
            DialogueDecision::NonDialogue,
            "局部及上下文复识别支持非台词，已自动隐藏；原始识别保留，可确认本句恢复",
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

enum Peer {
    Sense(SenseVoice),
    Gpu(super::sensevoice_gpu::GpuRecognizer),
    Fast(super::faster_whisper::FasterWhisper),
    Whisper(super::whisper_engine::Whisper),
}
impl Peer {
    fn transcribe(&mut self, audio: &[f32], cancel: &AtomicBool) -> AppResult<RecognitionEvidence> {
        match self {
            Self::Sense(e) => e.transcribe(audio),
            Self::Gpu(e) => e.transcribe(audio, cancel),
            Self::Fast(e) => e.transcribe_comparison(audio, cancel, true),
            Self::Whisper(e) => e.transcribe_comparison(audio, cancel, true),
        }
    }
}
pub struct Reviewer<'a> {
    service: &'a RecognitionService,
    peer: Option<(String, Peer)>,
    unavailable: bool,
    language: String,
    verifier: Option<(String, Peer)>,
    verifier_unavailable: bool,
    context_ms: i64,
}
impl<'a> Reviewer<'a> {
    pub fn new(service: &'a RecognitionService, language: &str) -> Self {
        Self {
            service,
            peer: None,
            unavailable: false,
            language: language.into(),
            verifier: None,
            verifier_unavailable: false,
            context_ms: 2500,
        }
    }
    pub fn set_language(&mut self, language: &str) {
        if self.language != language {
            self.peer = None;
            self.unavailable = false;
            self.verifier = None;
            self.verifier_unavailable = false;
            self.language = language.into();
        }
    }
    pub fn set_context_ms(&mut self, context_ms: i64) {
        self.context_ms = context_ms.clamp(2000, 8000);
    }
    fn prepare_verifier(&mut self, cancel: &AtomicBool) -> AppResult<()> {
        if self.verifier.is_some() || self.verifier_unavailable { return Ok(()); }
        // A distilled Kotoba decoder repeating its own answer is not an
        // independent check. Use an installed base Whisper, never download one.
        for name in ["faster_whisper", "whisper"] {
            cancelled(cancel)?;
            if self.service.require_engine_for(name).is_err() { continue; }
            let peer = if name == "whisper" {
                super::whisper_engine::Whisper::new(&self.service.whisper_directory(), &self.language, "").map(Peer::Whisper)
            } else {
                super::faster_whisper::FasterWhisper::new(
                    &self.service.ct2_directory(name)?, &self.service.sensevoice_fp16_directory(),
                    &self.language, "", cancel,
                ).map(Peer::Fast)
            };
            if let Ok(peer) = peer { self.verifier = Some((name.into(), peer)); return Ok(()); }
            cancelled(cancel)?;
        }
        self.verifier_unavailable = true;
        Ok(())
    }
    fn prepare(&mut self, primary: &str, cancel: &AtomicBool) -> AppResult<()> {
        cancelled(cancel)?;
        if self.peer.is_some() || self.unavailable {
            return Ok(());
        }
        let choices: &[&str] = if speech_family(primary) {
            &["faster_whisper", "whisper", "kotoba_whisper"]
        } else {
            &["sensevoice_fp16", "sensevoice"]
        };
        for &name in choices {
            if self.service.require_engine_for(name).is_err() {
                continue;
            }
            cancelled(cancel)?;
            let peer = match name {
                "sensevoice" => SenseVoice::new(
                    &self.service.runtime_directory(),
                    &self.service.model_directory(),
                    &self.language,
                )
                .map(Peer::Sense),
                "sensevoice_fp16" => super::sensevoice_gpu::GpuRecognizer::new(
                    &self.service.sensevoice_fp16_directory(),
                    &self.language,
                    cancel,
                )
                .map(Peer::Gpu),
                "whisper" => super::whisper_engine::Whisper::new(
                    &self.service.whisper_directory(),
                    &self.language,
                    "",
                )
                .map(Peer::Whisper),
                _ => super::faster_whisper::FasterWhisper::new(
                    &self.service.ct2_directory(name)?,
                    &self.service.sensevoice_fp16_directory(),
                    &self.language,
                    "",
                    cancel,
                )
                .map(Peer::Fast),
            };
            if let Ok(peer) = peer {
                self.peer = Some((name.into(), peer));
                return Ok(());
            }
            cancelled(cancel)?;
        }
        self.unavailable = true;
        Ok(())
    }
    pub fn review(
        &mut self,
        cue: &SubtitleCueDto,
        samples: &[f32],
        offset_ms: i64,
        primary: &str,
        cancel: &AtomicBool,
        mut transcribe: impl FnMut(&[f32]) -> AppResult<RecognitionEvidence>,
    ) -> AppResult<DialogueReview> {
        self.prepare(primary, cancel)?;
        let Some((peer_name, peer)) = self.peer.as_mut() else {
            let mut review = unavailable(
                "跨模型复核未就绪，原文保留；请在设置中安装另一类识别模型后重新识别",
                vec![],
            );
            review.context_ms = self.context_ms;
            return Ok(review);
        };
        let mut failed = false;
        let mut review = review_audio(
            cue,
            samples,
            offset_ms,
            primary,
            peer_name,
            cancel,
            |audio, use_primary| {
                let result = if use_primary {
                    transcribe(audio)
                } else {
                    peer.transcribe(audio, cancel)
                };
                failed |= result.is_err();
                result
            },
            self.context_ms,
        )?;
        review.context_ms = self.context_ms;
        if failed {
            self.peer = None;
            self.unavailable = true;
            return Ok(review);
        }
        if needs_independent_check(&cue.text, &review.observations) {
            self.prepare_verifier(cancel)?;
            if let Some((name, verifier)) = self.verifier.as_mut() {
                match observe(cue, samples, offset_ms, name, 0, cancel, |audio| verifier.transcribe(audio, cancel))? {
                    Some(observation) => review.observations.push(observation),
                    None => { self.verifier = None; self.verifier_unavailable = true; }
                }
            }
        }
        if needs_narrow_check(&cue.text, &review.observations) {
            let (peer_name, peer) = self.peer.as_mut().expect("prepared peer");
            let sense_primary = speech_family(primary);
            let name = if sense_primary { primary } else { peer_name.as_str() };
            match observe(cue, samples, offset_ms, name, 500, cancel, |audio| {
                if sense_primary { transcribe(audio) } else { peer.transcribe(audio, cancel) }
            })? {
                Some(observation) => review.observations.push(observation),
                None => { self.peer = None; self.unavailable = true; }
            }
        }
        review = decide(&cue.text, review.observations, Some(cue.end_ms));
        review.context_ms = self.context_ms;
        Ok(review)
    }
}

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
    fn extended_context_is_bounded_and_preference_changes_revisit_cached_candidates() {
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
        assert!(cached_candidates_for_context(&doc,Some(2000)).is_empty());
        assert_eq!(cached_candidates_for_context(&doc,Some(8000)),vec![7]);
        doc.asr_records.get_mut(&7).unwrap().dialogue_review.as_mut().unwrap().context_ms=8000;
        assert!(cached_candidates_for_context(&doc,Some(8000)).is_empty());
        doc.asr_records.get_mut(&7).unwrap().human_confirmed=true;
        assert!(cached_candidates_for_context(&doc,Some(2000)).is_empty());
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
    fn local_model_absence_never_downloads_or_calls_primary() {
        let root = tempfile::tempdir().unwrap();
        let db = crate::db::Database::open_and_migrate(
            &root.path().join("data"),
            &root.path().join("cache"),
        )
        .unwrap();
        let service = RecognitionService::new(db, root.path().join("data"), "test");
        let mut reviewer = Reviewer::new(&service, "ja");
        let result = reviewer
            .review(
                &cue(),
                &vec![0.0; 16000],
                0,
                "whisper",
                &AtomicBool::new(false),
                |_| panic!("no model"),
            )
            .unwrap();
        assert_eq!(result.decision, DialogueDecision::Unavailable);
        assert!(!root.path().join("data/models").exists());
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
        let mut primary = super::super::faster_whisper::FasterWhisper::new(
            &service.ct2_directory(&primary_name).unwrap(),
            &service.sensevoice_fp16_directory(),
            "ja",
            "",
            &cancel,
        )
        .unwrap();
        let mut reviewer = Reviewer::new(&service, "ja");
        if let Ok(executable) = std::env::var("ASMR_REVIEW_WORKER") {
            let peer=super::super::sensevoice_gpu::GpuRecognizer::for_local_test(
                std::path::Path::new(&executable), &service.sensevoice_fp16_directory(), "ja", &cancel,
            ).unwrap();
            // worker_main explicitly constructs the CUDA provider; its label is
            // the GPU product name, not necessarily a string containing "CUDA".
            println!("SenseVoice FP16 device: {}", peer.device);
            reviewer.peer=Some(("sensevoice_fp16".into(),Peer::Gpu(peer)));
        }
        if let Ok(context) = std::env::var("ASMR_REVIEW_CONTEXT_MS") { reviewer.set_context_ms(context.parse().unwrap()); }
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
                    |audio| primary.transcribe_comparison(audio, &cancel, true),
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
