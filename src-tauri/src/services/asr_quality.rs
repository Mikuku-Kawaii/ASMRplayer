//! Bounded audio checks. Suspicion is not proof of hallucination or a license to rewrite text.
use crate::domain::{asr::*, AppError, AppResult, ErrorCode};
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Instant,
};

pub struct Region<'a> {
    pub samples: &'a [f32],
    pub offset_ms: i64,
    pub begin: usize,
    pub end: usize,
    pub model: &'a str,
    pub language: &'a str,
    pub prompt: &'a str,
    pub context_samples: usize,
}
pub struct AssessedCue {
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
    pub record: AsrRecord,
}
fn cancelled(cancel: &AtomicBool) -> AppResult<()> {
    if cancel.load(Ordering::SeqCst) {
        Err(AppError::new(ErrorCode::MediaDecodeFailed, "已取消识别"))
    } else {
        Ok(())
    }
}
pub fn activity(samples: &[f32]) -> AudioActivity {
    if samples.is_empty() {
        return AudioActivity::default();
    }
    let rms =
        |s: &[f32]| (s.iter().map(|v| (*v as f64).powi(2)).sum::<f64>() / s.len() as f64).sqrt();
    AudioActivity {
        rms: rms(samples),
        peak: samples.iter().map(|v| v.abs() as f64).fold(0., f64::max),
        nonzero_fraction: samples.iter().filter(|v| **v != 0.).count() as f64
            / samples.len() as f64,
        active_frame_fraction: samples.chunks(320).filter(|s| rms(s) > 0.00005).count() as f64
            / samples.len().div_ceil(320) as f64,
    }
}
fn compact(text: &str) -> String {
    text.chars().filter(|c| c.is_alphanumeric()).collect()
}
pub fn repetitive(text: &str) -> bool {
    let chars: Vec<_> = compact(text).chars().collect();
    // Only long loops are diagnostic; real repeated short phrases remain intact.
    (1..=64).any(|width| {
        let repeats = if width >= 4 { 3 } else { 6 };
        chars.len() >= width * repeats
            && chars
                .windows(width * repeats)
                .any(|w| w.chunks(width).all(|chunk| chunk == &w[..width]))
    })
}
fn repetition_ratio(text: &str) -> Option<f64> {
    let chars: Vec<_> = compact(text).chars().collect();
    if chars.is_empty() {
        return None;
    }
    let mut longest = 0;
    for width in 1..=64.min(chars.len() / 3) {
        let mut repeated = 0;
        for i in width..chars.len() {
            if chars[i] == chars[i - width] {
                repeated += 1;
            } else {
                repeated = 0;
            }
            if repeated >= 2 * width {
                longest = longest.max(repeated + width);
            }
        }
    }
    Some(longest as f64 / chars.len() as f64)
}
fn signals(
    result: &RecognitionEvidence,
    audio: &AudioActivity,
    language: &str,
    prompt: &str,
) -> Vec<String> {
    let mut found = Vec::new();
    let text = super::speech_engine::clean_text(&result.text);
    if text.trim().is_empty() {
        found.push("empty_result".into());
    }
    if repetitive(&text) {
        found.push("repetition".into());
    }
    if language == "ja" {
        let kana = text.chars().any(|c| ('\u{3040}'..='\u{30ff}').contains(&c));
        let foreign = text
            .chars()
            .filter(|c| {
                c.is_ascii_alphabetic()
                    || ('\u{0400}'..='\u{052f}').contains(c)
                    || ('\u{ac00}'..='\u{d7af}').contains(c)
            })
            .count();
        if (!kana && foreign >= 4 || foreign >= 12 && foreign * 5 > text.chars().count() * 3)
            || result.detected_language.as_deref().is_some_and(|l| {
                let l = l.trim_start_matches("<|").trim_end_matches("|>");
                !l.is_empty() && l != "ja" && l != "auto"
            })
        {
            found.push("unexpected_language".into());
        }
    }
    let normalized = compact(&text);
    let copied = prompt
        .lines()
        .filter(|line| {
            let term = compact(line);
            term.chars().count() >= 2 && normalized.contains(&term)
        })
        .count();
    if copied >= 3 {
        found.push("prompt_echo".into());
    }
    // Multiple decoder metrics constitute one family of evidence, not independent votes.
    if result.segments.iter().any(|s| {
        s.avg_logprob.is_some_and(|p| p < -1.0)
            && (s.no_speech_prob.is_some_and(|p| p > 0.6)
                || s.compression_ratio.is_some_and(|r| r > 2.4))
    }) {
        found.push("decoder_uncertain".into());
    }
    if audio.active_frame_fraction < 0.1 {
        found.push("low_activity".into());
    }
    found
}
fn reliable_segments(result: &RecognitionEvidence, duration: i64) -> bool {
    !result.segments.is_empty()
        && result.segments.iter().all(|s| {
            s.start.is_finite()
                && s.end.is_finite()
                && s.start >= 0.
                && s.end > s.start
                && s.end * 1000. <= duration as f64 + 20.
        })
        && result
            .segments
            .windows(2)
            .all(|s| s[0].end <= s[1].start + 0.02)
        && compact(
            &result
                .segments
                .iter()
                .map(|s| s.text.as_str())
                .collect::<String>(),
        ) == compact(&result.text)
}
/// SenseVoice token onsets are not per-character alignment. Only split at an
/// actual PCM pause AND a tokenizer word boundary; never interpolate within a token.
fn token_pause_segments(result: &mut RecognitionEvidence, samples: &[f32]) {
    if !result.segments.is_empty()
        || result.tokens.len() != result.timestamps.len()
        || result.tokens.is_empty()
        || result
            .timestamps
            .iter()
            .any(|t| !t.is_finite() || *t < 0. || *t * 16000. >= samples.len() as f64)
        || result.timestamps.windows(2).any(|t| t[0] > t[1])
    {
        return;
    }
    let token_text = |s: &str| s.replace('▁', " ");
    let joined = result
        .tokens
        .iter()
        .map(|s| token_text(s))
        .collect::<String>();
    if compact(&joined) != compact(&super::speech_engine::clean_text(&result.text)) {
        return;
    }
    let mut boundaries = vec![(0, 0usize)];
    for i in 1..result.tokens.len() {
        if !result.tokens[i].starts_with('▁') {
            continue;
        }
        let from = (result.timestamps[i - 1] * 16000.) as usize;
        let to = (result.timestamps[i] * 16000.) as usize;
        if to.saturating_sub(from) < 3840 {
            continue;
        }
        if let Some(pause) = (from..to.saturating_sub(3840))
            .step_by(320)
            .find(|p| samples[*p..*p + 3840].iter().all(|s| *s == 0.))
        {
            boundaries.push((i, pause + 1920));
        }
    }
    if boundaries.len() == 1 {
        return;
    }
    boundaries.push((result.tokens.len(), samples.len()));
    for pair in boundaries.windows(2) {
        result.segments.push(SegmentEvidence {
            start: pair[0].1 as f64 / 16000.,
            end: pair[1].1 as f64 / 16000.,
            text: result.tokens[pair[0].0..pair[1].0]
                .iter()
                .map(|s| token_text(s))
                .collect::<String>()
                .trim()
                .into(),
            ..Default::default()
        });
    }
    result.config["timing"] = "token_boundary_and_digital_pause".into();
}
fn attempt(
    region: &Region<'_>,
    begin: usize,
    end: usize,
    without_prompt: bool,
    cancel: &AtomicBool,
    transcribe: &mut impl FnMut(&[f32], bool) -> AppResult<RecognitionEvidence>,
) -> AppResult<AsrAttempt> {
    cancelled(cancel)?;
    let started = Instant::now();
    let audio = activity(&region.samples[begin..end]);
    let mut result = transcribe(&region.samples[begin..end], without_prompt)?;
    cancelled(cancel)?;
    // Backend-owned settings accompany every result. Unsupported metrics remain None.
    if result.config.is_null() {
        result.config = if region.model.starts_with("sensevoice") {
            serde_json::json!({"backend":"sherpa-onnx-1.12.20","provider":if region.model == "sensevoice_fp16" {"cuda"} else {"cpu"},"language":region.language,"sample_rate":16000,"feature_dim":80,"decoding_method":"greedy_search","use_itn":true,"num_threads":2,"vad":false,"text_prompt_supported":false,"decoder_diagnostics":null})
        } else {
            serde_json::json!({"backend":region.model,"decoder_diagnostics":null})
        };
    }
    token_pause_segments(&mut result, &region.samples[begin..end]);
    let prompt = result
        .config
        .get("initial_prompt")
        .and_then(|p| p.as_str())
        .unwrap_or(if without_prompt { "" } else { region.prompt });
    let signals = signals(&result, &audio, region.language, prompt);
    let repetition_ratio = repetition_ratio(&result.text);
    Ok(AsrAttempt {
        start_ms: region.offset_ms + begin as i64 / 16,
        end_ms: region.offset_ms + end as i64 / 16,
        prompt: prompt.into(),
        elapsed_ms: started.elapsed().as_millis() as u64,
        audio,
        result,
        signals,
        repetition_ratio,
    })
}
pub fn recognize(
    region: Region<'_>,
    cancel: &AtomicBool,
    mut transcribe: impl FnMut(&[f32], bool) -> AppResult<RecognitionEvidence>,
) -> AppResult<Vec<AssessedCue>> {
    cancelled(cancel)?;
    if region.begin >= region.end
        || region.end > region.samples.len()
        || region.end - region.begin > 192000
        || region.offset_ms < 0
        || region.samples.iter().any(|sample| !sample.is_finite())
    {
        return Err(AppError::new(
            ErrorCode::MediaDecodeFailed,
            "未能读取有效的待识别音频区间",
        ));
    }
    let start_ms = region.offset_ms + region.begin as i64 / 16;
    let end_ms = region.offset_ms + region.end as i64 / 16;
    let owned = &region.samples[region.begin..region.end];
    // Exact digital silence is the only automatic no-speech proof available here.
    // Noise and very quiet speech cannot be distinguished by RMS alone.
    if owned.iter().all(|sample| *sample == 0.) {
        return Ok(vec![AssessedCue {
            start_ms,
            end_ms,
            text: String::new(),
            record: AsrRecord {
                dialogue_review_history: vec![],
                dialogue_review: None,
                model: region.model.into(),
                language: region.language.into(),
                selected_attempt: 0,
                pending_audio_review: false,
                confirmed_no_speech: true,
                human_confirmed: false,
                note: "数字静音：该区间全部 PCM 样本为零".into(),
                attempts: vec![AsrAttempt {
                    start_ms,
                    end_ms,
                    prompt: region.prompt.into(),
                    elapsed_ms: 0,
                    audio: activity(owned),
                    result: RecognitionEvidence {
                        config: serde_json::json!({"inference":"skipped_exact_silence"}),
                        ..Default::default()
                    },
                    signals: vec!["digital_silence".into()],
                    repetition_ratio: None,
                }],
            },
        }]);
    }
    let initial = attempt(
        &region,
        region.begin,
        region.end,
        false,
        cancel,
        &mut transcribe,
    )?;
    let suspicious = initial.signals.len() >= 2;
    let empty = initial.result.text.trim().is_empty();
    let timing_problem = !initial.result.segments.is_empty()
        && !reliable_segments(&initial.result, initial.end_ms - initial.start_ms);
    let fragment = compact(&initial.result.text).chars().count() <= 3;
    let boundary = region.begin > 0
        && region.samples[region.begin.saturating_sub(3840)..region.begin]
            .iter()
            .any(|v| *v != 0.)
        || region.end < region.samples.len()
            && region.samples[region.end..(region.end + 3840).min(region.samples.len())]
                .iter()
                .any(|v| *v != 0.);
    let mut attempts = vec![initial];
    let mut selected = 0;
    let mut retry_failed = false;
    let mut context_spill = false;
    if suspicious || empty || fragment || boundary || timing_problem {
        // The PCM cache may be wider, but every backend request stays <= 20s.
        let padding = region.context_samples.min(128000)
            .min(320000usize.saturating_sub(region.end-region.begin) / 2);
        let begin = region
            .begin
            .saturating_sub(padding);
        let end = (region.end + padding).min(region.samples.len());
        let retry_started = Instant::now();
        match attempt(&region, begin, end, true, cancel, &mut transcribe) {
            Ok(mut retry) => {
                let reliable = reliable_segments(&retry.result, retry.end_ms - retry.start_ms);
                let ownership: Vec<_> = retry.result.segments.iter().map(|s| {
                    let begin = retry.start_ms + (s.start * 1000.) as i64;
                    let end = retry.start_ms + (s.end * 1000.) as i64;
                    let initial = compact(&attempts[0].result.text);
                    let anchored = initial.chars().count() >= 2 && compact(&s.text).contains(&initial);
                    (begin < end_ms && end > start_ms,
                        begin >= start_ms && end <= end_ms || anchored)
                }).collect();
                let owns = ownership.iter().any(|(overlaps, anchored)| *overlaps && *anchored);
                let spills = ownership.iter().any(|(overlaps, anchored)| *overlaps && !*anchored);
                if reliable && (!owns || spills) && !retry.result.text.trim().is_empty() {
                    context_spill = true;
                    retry.signals.push("context_outside_owned_audio".into());
                }
                // A changed result is usable only with decoder timing and fewer risk signals.
                // Agreement alone does not clear an existing suspicion.
                if reliable
                    && owns
                    && !spills
                    && !retry.result.text.trim().is_empty()
                    && retry.signals.len() < attempts[0].signals.len().max(1)
                    && (suspicious || empty || fragment || timing_problem)
                {
                    selected = 1;
                }
                attempts.push(retry);
            }
            Err(err) if !cancel.load(Ordering::SeqCst) => {
                retry_failed = true;
                attempts.push(AsrAttempt {
                    start_ms: region.offset_ms+begin as i64/16, end_ms:region.offset_ms+end as i64/16,
                    prompt:String::new(), elapsed_ms:retry_started.elapsed().as_millis() as u64,
                    audio:activity(&region.samples[begin..end]),
                    result:RecognitionEvidence {config:serde_json::json!({"inference_error":err.message,"backend":region.model}),..Default::default()},
                    signals:vec!["retry_failed".into()],repetition_ratio:None,
                });
            }
            Err(e) => return Err(e),
        }
    }
    let chosen = &attempts[selected];
    let timed = reliable_segments(&chosen.result, chosen.end_ms - chosen.start_ms);
    let unresolved = suspicious
        || context_spill
        || chosen.result.text.trim().is_empty()
        || retry_failed
        || (fragment || boundary) && !timed
        || !chosen.result.segments.is_empty() && !timed
        || chosen.signals.iter().any(|s| {
            s == "unexpected_language"
                || s == "repetition"
                || s == "decoder_uncertain"
                || s == "prompt_echo"
        });
    let note = if retry_failed {
        "局部复识别失败，原始结果已保留；请听音核对"
    } else if unresolved {
        "局部复识别后仍有疑点或缺少可靠时间依据；原始识别与对照结果已保留，请听音核对"
    } else {
        ""
    };
    let record = AsrRecord {
        dialogue_review_history: vec![],
        dialogue_review: None,
        model: region.model.into(),
        language: region.language.into(),
        attempts: attempts.clone(),
        selected_attempt: selected,
        pending_audio_review: unresolved,
        confirmed_no_speech: false,
        human_confirmed: false,
        note: note.into(),
    };
    let mut cues = Vec::new();
    if timed {
        for segment in &chosen.result.segments {
            let start = chosen.start_ms + (segment.start * 1000.).round() as i64;
            let end = chosen.start_ms + (segment.end * 1000.).round() as i64;
            if end <= start_ms || start >= end_ms {
                continue;
            }
            cues.push(AssessedCue {
                start_ms: start,
                end_ms: end,
                text: super::speech_engine::clean_text(&segment.text),
                record: record.clone(),
            });
        }
    }
    if cues.is_empty() {
        cues.push(AssessedCue {
            start_ms,
            end_ms,
            text: super::speech_engine::clean_text(&attempts[0].result.text),
            record,
        });
    }
    Ok(cues)
}

/// Decode every sample, splitting at digital pauses where possible. Energy never rejects a region.
pub fn regions(samples: &[f32]) -> Vec<(usize, usize)> {
    let mut result = Vec::new();
    let mut begin = 0;
    while begin < samples.len() {
        let limit = (begin + 12 * 16000).min(samples.len());
        let end = if limit == samples.len() {
            limit
        } else {
            // A zero run is a boundary hint only. Retain it in one of the adjacent windows.
            (begin + 6 * 16000..limit)
                .rev()
                .step_by(320)
                .find(|end| {
                    samples[end.saturating_sub(3840)..*end]
                        .iter()
                        .all(|s| *s == 0.)
                })
                .unwrap_or(limit)
        };
        result.push((begin, end));
        begin = end;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn extended_retry_adds_context_without_exceeding_backend_budget() {
        let audio=vec![0.00001;40*16000];
        for duration in [2,12] {
            let mut input=region(&audio);
            input.begin=8*16000;input.end=(8+duration)*16000;input.context_samples=8*16000;
            let mut lengths=vec![];
            recognize(input,&AtomicBool::new(false),|samples,_| {
                lengths.push(samples.len());Ok(text(""))
            }).unwrap();
            assert_eq!(lengths,vec![duration*16000,((duration+16).min(20))*16000]);
        }
    }
    fn region(audio: &[f32]) -> Region<'_> {
        Region {
            samples: audio,
            offset_ms: 0,
            begin: 0,
            end: audio.len(),
            model: "fixture",
            language: "ja",
            prompt: "",
            context_samples: 32000,
        }
    }
    fn text(value: &str) -> RecognitionEvidence {
        RecognitionEvidence {
            text: value.into(),
            ..Default::default()
        }
    }
    fn timed(value: &str, start: f64, end: f64) -> RecognitionEvidence {
        RecognitionEvidence {
            segments: vec![SegmentEvidence {
                start,
                end,
                text: value.into(),
                ..Default::default()
            }],
            ..text(value)
        }
    }
    #[test]
    fn digital_silence_is_explicit_and_does_not_invoke_any_model() {
        let audio = vec![0.; 16000];
        let output = recognize(region(&audio), &AtomicBool::new(false), |_, _| {
            panic!("silent audio must not decode")
        })
        .unwrap();
        assert!(output[0].record.confirmed_no_speech);
        assert!(!output[0].record.pending_audio_review);
        assert!(output[0].text.is_empty());
        let json = serde_json::to_value(&output[0].record).unwrap();
        assert!(json["attempts"][0]["result"]["detected_language"].is_null());
    }
    #[test]
    fn noise_without_a_transcript_is_retained_for_review_once_not_labeled_silence() {
        // Synthetic alternating friction-like waveform, not a claim of real noise classification.
        let audio: Vec<_> = (0..16000)
            .map(|i| if i % 2 == 0 { 0.00001 } else { -0.00001 })
            .collect();
        let mut calls = 0;
        let output = recognize(region(&audio), &AtomicBool::new(false), |_, _| {
            calls += 1;
            Ok(text(""))
        })
        .unwrap();
        assert_eq!(calls, 2);
        assert!(!output[0].record.confirmed_no_speech);
        assert!(output[0].record.pending_audio_review);
        assert_eq!(output[0].record.attempts.len(), 2);
    }
    #[test]
    fn actual_boilerplate_and_normal_dialogue_are_not_blacklisted() {
        assert!(repetitive(&"ご視聴ありがとうございました".repeat(3)));
        assert!(!repetitive("ご視聴ありがとうございました"));
        for phrase in [
            "ご視聴ありがとうございました",
            "ごちそうさまでした",
            "君には特別にしてあげたいなって",
        ] {
            let audio = vec![0.01; 16000];
            let mut calls = 0;
            let output = recognize(region(&audio), &AtomicBool::new(false), |_, _| {
                calls += 1;
                Ok(timed(phrase, 0., 1.))
            })
            .unwrap();
            assert_eq!(output[0].text, phrase);
            assert!(!output[0].record.pending_audio_review);
            assert_eq!(calls, 1);
        }
    }
    #[test]
    fn weak_speech_and_brief_hums_are_never_deleted_for_volume_or_length() {
        for (amplitude, length, phrase) in [
            (0.000001, 16000, "おやすみなさい"),
            (0.0000001, 800, "んー"),
        ] {
            let audio = vec![amplitude; length];
            assert_eq!(regions(&audio), vec![(0, length)]);
            let output = recognize(region(&audio), &AtomicBool::new(false), |_, _| {
                Ok(text(phrase))
            })
            .unwrap();
            assert_eq!(output[0].text, phrase);
            assert!(!output[0].record.confirmed_no_speech);
        }
    }
    #[test]
    fn suspicious_foreign_loop_is_not_deleted_even_when_comparison_agrees() {
        let audio = vec![0.00001; 16000];
        let mut calls = 0;
        let output = recognize(region(&audio), &AtomicBool::new(false), |_, _| {
            calls += 1;
            Ok(text("hello hello hello hello hello hello"))
        })
        .unwrap();
        assert_eq!(calls, 2);
        assert!(output[0].record.pending_audio_review);
        assert!(!output[0].record.confirmed_no_speech);
        assert!(output[0].text.contains("hello"));
        assert!(output[0].record.attempts[0]
            .signals
            .contains(&"unexpected_language".into()));
    }
    #[test]
    fn decoder_doubt_and_audio_activity_are_combined_but_preserved() {
        let audio = vec![0.00001; 16000];
        let mut result = timed("ご視聴ありがとうございました", 0., 1.);
        result.segments[0].avg_logprob = Some(-1.5);
        result.segments[0].no_speech_prob = Some(0.9);
        let output = recognize(region(&audio), &AtomicBool::new(false), |_, _| {
            Ok(result.clone())
        })
        .unwrap();
        assert_eq!(output[0].record.attempts.len(), 2);
        assert_eq!(
            output[0].record.attempts[0].result.segments[0].compression_ratio,
            None
        );
        assert!(output[0].record.pending_audio_review);
        assert_eq!(output[0].text, result.text);
    }
    #[test]
    fn expanded_context_cannot_move_a_new_sentence_into_the_previous_owned_chunk() {
        let audio = vec![0.01; 14 * 16000];
        let mut input = region(&audio);
        input.offset_ms = 409000;
        input.begin = 3 * 16000;
        input.end = 11 * 16000;
        input.context_samples = 3 * 16000;
        let mut calls = 0;
        let cues = recognize(input, &AtomicBool::new(false), |_,_| {
            calls += 1;
            Ok(if calls == 1 { timed("ああ",0.,4.64) } else { timed("ちょっと痛かった?",0.,12.4) })
        }).unwrap();
        assert_eq!(calls,2);
        assert_eq!(cues[0].text,"ああ");
        assert_eq!(cues[0].start_ms,412000);
        assert_eq!(cues[0].record.selected_attempt,0);
        assert!(cues[0].record.pending_audio_review);
        assert_eq!(cues[0].record.attempts[1].result.text,"ちょっと痛かった?");
        assert!(cues[0].record.attempts[1].signals.iter().any(|s| s == "context_outside_owned_audio"));
    }
    #[test]
    fn sustained_word_recovery_uses_audio_context_and_decoder_time_not_a_word_patch() {
        let audio = vec![0.01; 64000];
        let mut input = region(&audio);
        input.begin = 16000;
        input.end = 48000;
        input.prompt = "作品の名前";
        let mut calls = Vec::new();
        let output = recognize(input, &AtomicBool::new(false), |samples, no_prompt| {
            calls.push((samples.len(), no_prompt));
            Ok(if calls.len() == 1 {
                text("おや")
            } else {
                timed("おやすみ", 0.5, 3.5)
            })
        })
        .unwrap();
        assert_eq!(calls, vec![(32000, false), (64000, true)]);
        assert_eq!(output[0].text, "おやすみ");
        assert_eq!((output[0].start_ms, output[0].end_ms), (500, 3500));
        assert_eq!(output[0].record.attempts[0].result.text, "おや");
        assert_eq!(output[0].record.selected_attempt, 1);
    }
    #[test]
    fn changed_comparison_without_alignment_does_not_invent_a_repaired_word_or_time() {
        let audio = vec![0.01; 32000];
        let mut calls = 0;
        let output = recognize(region(&audio), &AtomicBool::new(false), |_, _| {
            calls += 1;
            Ok(text(if calls == 1 { "おや" } else { "おやすみ" }))
        })
        .unwrap();
        assert_eq!(output[0].text, "おや");
        assert_eq!((output[0].start_ms, output[0].end_ms), (0, 2000));
        assert!(output[0].record.pending_audio_review);
    }
    #[test]
    fn spaces_need_token_and_audio_pause_evidence_and_never_split_inside_a_token() {
        let mut result = RecognitionEvidence {
            text: "残念でした 君には".into(),
            tokens: vec!["残念でした".into(), "▁君には".into()],
            timestamps: vec![0., 1.],
            config: serde_json::json!({}),
            ..Default::default()
        };
        let mut audio = vec![0.01; 32000];
        token_pause_segments(&mut result, &audio);
        assert!(result.segments.is_empty());
        audio[8000..15000].fill(0.);
        token_pause_segments(&mut result, &audio);
        assert_eq!(result.segments.len(), 2);
        assert_eq!(result.segments[1].text, "君には");
        assert_eq!(result.segments[0].end, result.segments[1].start);
        let mut one = text("残念でした 君には");
        one.tokens = vec![one.text.clone()];
        one.timestamps = vec![0.];
        token_pause_segments(&mut one, &audio);
        assert!(one.segments.is_empty());
    }
    #[test]
    fn finite_retry_and_cancel_retain_the_first_result_on_comparison_error() {
        let audio = vec![0.01; 16000];
        let cancel = AtomicBool::new(false);
        let mut calls = 0;
        let output = recognize(region(&audio), &cancel, |_, _| {
            calls += 1;
            if calls == 1 {
                Ok(text("み"))
            } else {
                Err(AppError::new(
                    ErrorCode::MediaDecodeFailed,
                    "fixture failure",
                ))
            }
        })
        .unwrap();
        assert_eq!(calls, 2);
        assert_eq!(output[0].text, "み");
        assert!(output[0].record.pending_audio_review);
        let cancelled_flag = AtomicBool::new(true);
        assert!(recognize(region(&audio), &cancelled_flag, |_, _| panic!("cancelled")).is_err());
        assert!(recognize(region(&[]), &cancel, |_, _| panic!("invalid input")).is_err());
    }
    #[test]
    fn segmentation_has_no_unowned_samples_or_volume_gate() {
        let mut audio = vec![0.0000001; 320000];
        audio[140000..150000].fill(0.);
        let chunks = regions(&audio);
        assert_eq!(chunks[0].0, 0);
        assert_eq!(chunks.last().unwrap().1, audio.len());
        assert!(chunks.windows(2).all(|p| p[0].1 == p[1].0));
        assert!(chunks.iter().all(|(a, b)| b - a <= 192000));
        assert_eq!(
            chunks.iter().map(|(a, b)| b - a).sum::<usize>(),
            audio.len()
        );
    }
    #[test]
    fn prompt_echo_requires_multiple_terms_and_comparison_removes_the_prompt() {
        let audio = vec![0.000001; 16000];
        let mut input = region(&audio);
        input.prompt = "花子\n先生\n学校";
        let mut prompts = Vec::new();
        let output = recognize(input, &AtomicBool::new(false), |_, without| {
            prompts.push(without);
            Ok(text("花子先生学校"))
        })
        .unwrap();
        assert_eq!(prompts, vec![false, true]);
        assert!(output[0].record.attempts[0]
            .signals
            .contains(&"prompt_echo".into()));
        assert!(output[0].record.pending_audio_review);
        assert_eq!(output[0].record.attempts[1].prompt, "");
    }
    #[test]
    fn states_survive_serialization_and_do_not_turn_review_or_silence_into_translation_jobs() {
        use crate::{db::subtitles::SubtitleDocument, domain::SubtitleCueDto};
        let audio = vec![0.00001; 16000];
        let mut output =
            recognize(region(&audio), &AtomicBool::new(false), |_, _| Ok(text(""))).unwrap();
        let mut doc = SubtitleDocument::new(
            "v".into(),
            "sensevoice:fixture".into(),
            vec![SubtitleCueDto {
                index: 9,
                start_ms: 0,
                end_ms: 1000,
                text: String::new(),
            }],
            true,
        );
        doc.asr_records.insert(9, output.remove(0).record);
        doc.completed_chunks.insert(0);
        doc.validate().unwrap();
        let mut doc: SubtitleDocument =
            serde_json::from_slice(&serde_json::to_vec(&doc).unwrap()).unwrap();
        assert_eq!(doc.cue_status(9), "audioReview");
        assert!(!doc.can_translate(9));
        assert!(super::super::recognition::full_text_translation::review_input(&doc).is_empty());
        assert!(doc.completed_chunks.contains(&0));
        doc.original_corrections.insert(9, "おやすみ".into());
        doc.asr_records.get_mut(&9).unwrap().human_confirmed = true;
        assert!(doc.can_translate(9));
        assert_eq!(doc.cue_status(9), "pendingTranslation");
        doc.translation_failures.insert(9, "fixture timeout".into());
        assert_eq!(doc.cue_status(9), "translationFailed");
        doc.translation_corrections.insert(9, "人工晚安".into());
        assert_eq!(doc.cue_status(9), "complete");
        doc.translations.insert(9, "自动结果".into());
        assert_eq!(doc.translation_text(9), Some("人工晚安"));
        let silence = recognize(
            region(&vec![0.; 16000]),
            &AtomicBool::new(false),
            |_, _| unreachable!(),
        )
        .unwrap();
        doc.asr_records.insert(9, silence[0].record.clone());
        assert_eq!(doc.cue_status(9), "noSpeech");
        assert!(!doc.can_translate(9));
        assert!(doc.rendered().is_empty());
    }
    #[test]
    fn display_sound_limit_does_not_mutate_raw_text_or_real_repeated_words() {
        use crate::{db::subtitles::SubtitleDocument, domain::SubtitleCueDto};
        let original = "こんにちは フッフッフッフッフッ またね";
        let doc = SubtitleDocument::new(
            "v".into(),
            "sensevoice:fixture".into(),
            vec![SubtitleCueDto {
                index: 3,
                start_ms: 0,
                end_ms: 5000,
                text: original.into(),
            }],
            true,
        );
        let displayed = doc.rendered();
        assert_eq!(displayed.len(), 1);
        assert_eq!(displayed[0].text, "こんにちは またね");
        assert_eq!((displayed[0].start_ms, displayed[0].end_ms), (0, 5000));
        assert_eq!(doc.original[0].text, original);
        assert_eq!(
            crate::domain::asr::display(&format!("{}{}", "フッ".repeat(5), "ヘ".repeat(200))),
            "フッフッフッ\nヘヘヘ"
        );
        for ordinary in [
            "ごめんごめんごめんごめん",
            "おやすーーーーみ",
            "ああああああ",
            "はは母の話です",
        ] {
            assert_eq!(crate::domain::asr::display(ordinary), ordinary);
        }
    }
}
