//! Conservative text-only review, traceable bounded context, host-owned IDs and timing.
use super::*;
use crate::db::subtitles::{ModelReviewRecord, SubtitleDocument};
use std::collections::BTreeMap;

#[derive(Clone, Serialize)]
pub(crate) struct ReviewInput {
    pub id: usize,
    pub original: String,
    pub locked: bool,
    pub chinese_reference: Option<String>,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct ReviewedLine {
    pub id: usize,
    pub original: String,
    #[serde(skip)]
    pub translation: String,
    pub parts: Vec<serde_json::Value>,
    #[serde(default)]
    pub uncertain: bool,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub phonetic_reason: String,
    #[serde(default)]
    pub context_reason: String,
    #[serde(skip)]
    trace: ModelReviewRecord,
}
pub(crate) fn review_input(doc: &SubtitleDocument) -> Vec<ReviewInput> {
    doc.original
        .iter()
        .filter(|cue| doc.can_translate(cue.index))
        .map(|cue| ReviewInput {
            id: cue.index,
            original: doc.dialogue_text(cue),
            locked: doc.original_corrections.contains_key(&cue.index)
                || doc.rule_corrections.contains_key(&cue.index),
            chinese_reference: doc
                .translation_corrections
                .get(&cue.index)
                .cloned()
                .or_else(|| doc.trusted_reference(cue.index).map(str::to_owned)),
        })
        .collect()
}
fn parse_review(
    content: &str,
    inputs: &[ReviewInput],
    settings: &RecognitionSettings,
) -> AppResult<BTreeMap<usize, ReviewedLine>> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Output {
        subtitles: Vec<serde_json::Value>,
    }
    let output: Output =
        serde_json::from_str(content).map_err(|_| invalid("校对结果不是有效 JSON"))?;
    let mut result = BTreeMap::new();
    for input in inputs {
        let matching: Vec<_> = output
            .subtitles
            .iter()
            .filter(|line| line["id"].as_u64() == Some(input.id as u64))
            .collect();
        if matching.len() != 1 {
            continue;
        }
        let Ok(mut line) = serde_json::from_value::<ReviewedLine>(matching[0].clone()) else {
            continue;
        };
        line.original = line.original.trim().into();
        let translations = super::deepseek_translation::covered_translations(
            &serde_json::json!({"translations":[{"id":line.id,"parts":line.parts}]}).to_string(),
            &[TranslationLine {
                id: line.id,
                text: line.original.clone(),
            }],
            settings,
        );
        let Some(translation) = translations.get(&line.id) else {
            continue;
        };
        line.translation = translation.clone();
        let changed = line.original != input.original;
        if line.original.is_empty()
            || line.original.len() > 100_000
            || line.original.contains('\0')
            || validate_translation_text(&line.translation).is_err()
            || (input.locked && changed)
            || (line.uncertain && changed)
            || (changed
                && (line.phonetic_reason.trim().is_empty()
                    || line.context_reason.trim().is_empty()))
            || [&line.note, &line.phonetic_reason, &line.context_reason]
                .iter()
                .any(|text| text.len() > 1500 || text.contains('\0'))
        {
            continue;
        }
        // Text models cannot confirm audio. Even supported corrections remain reviewable suggestions.
        if changed {
            line.trace.note = format!(
                "文字候选，尚未经听音确认。发音关系：{}；语境依据：{}",
                line.phonetic_reason, line.context_reason
            );
        } else if line.uncertain || !line.note.trim().is_empty() {
            line.trace.note = if line.note.trim().is_empty() {
                "原文存疑，已保留；请听音核对。".into()
            } else {
                line.note.clone()
            };
        }
        result.insert(line.id, line);
    }
    Ok(result)
}
pub(crate) fn apply_review(doc: &mut SubtitleDocument, result: BTreeMap<usize, ReviewedLine>) {
    for (id, mut line) in result {
        if !doc.original.iter().any(|cue| cue.index == id) {
            continue;
        }
        // Re-translating a candidate is not human confirmation of the earlier correction.
        if line.trace.note.is_empty() && doc.model_corrections.get(&id) == Some(&line.original) {
            if let Some(previous) = doc.model_reviews.get(&id) {
                line.trace.note = previous.note.clone();
            }
        }
        if !doc.original_corrections.contains_key(&id) && !doc.rule_corrections.contains_key(&id) {
            doc.model_corrections.insert(id, line.original);
        }
        doc.model_reviews.insert(id, line.trace);
        if !doc.translation_corrections.contains_key(&id) && doc.trusted_reference(id).is_none() {
            doc.translations.insert(id, line.translation);
        }
    }
}

// Whole cues only. Every requested target is retained, neighbours are explicitly scoped.
// Byte budget is conservative for UTF-8/tokenisation, and reserves output/system space.
fn context_window<'a>(
    transcript: &'a [ReviewInput],
    targets: &[usize],
    budget: usize,
) -> AppResult<&'a [ReviewInput]> {
    let positions: Vec<_> = transcript
        .iter()
        .enumerate()
        .filter(|(_, line)| targets.contains(&line.id))
        .map(|(i, _)| i)
        .collect();
    if positions.len() != targets.len() || positions.is_empty() {
        return Err(invalid("校对字幕编号不一致"));
    }
    let size = |lines: &[ReviewInput]| {
        serde_json::to_vec(lines)
            .map(|bytes| bytes.len())
            .unwrap_or(usize::MAX)
    };
    if size(transcript) <= budget {
        return Ok(transcript);
    }
    let (mut first, mut end) = (positions[0], positions[positions.len() - 1] + 1);
    if size(&transcript[first..end]) > budget {
        return Err(invalid("单次目标字幕超过校对上下文预算，未截断原文"));
    }
    loop {
        let before = (first, end);
        if first > 0 && size(&transcript[first - 1..end]) <= budget {
            first -= 1;
        }
        if end < transcript.len() && size(&transcript[first..end + 1]) <= budget {
            end += 1;
        }
        if before == (first, end) {
            break;
        }
    }
    Ok(&transcript[first..end])
}

impl RecognitionService {
    pub(crate) async fn review_full_text(
        &self,
        settings: &RecognitionSettings,
        transcript: &[ReviewInput],
        targets: &[usize],
    ) -> AppResult<BTreeMap<usize, ReviewedLine>> {
        let _lease = self
            .model_access
            .try_read()
            .map_err(|_| invalid("正在删除模型，请稍后翻译"))?;
        let settings = validate_settings(settings.clone())?;
        if targets.is_empty() || targets.len() > LOCAL_BATCH_SIZE {
            return Err(invalid("全文校对批次无效"));
        }
        let mut accepted = BTreeMap::new();
        let mut failure = None;
        for attempt in 0..2 {
            let pending: Vec<_> = targets
                .iter()
                .copied()
                .filter(|id| !accepted.contains_key(id))
                .collect();
            let width = if attempt == 0 { LOCAL_BATCH_SIZE } else { 1 };
            for group in pending.chunks(width) {
                match self.review_request(&settings, transcript, group).await {
                    Ok(lines) => accepted.extend(lines),
                    Err(err) => failure = Some(err),
                }
            }
        }
        if accepted.is_empty() {
            if let Some(err) = failure {
                return Err(err);
            }
        }
        Ok(accepted)
    }
    async fn review_request(
        &self,
        settings: &RecognitionSettings,
        transcript: &[ReviewInput],
        targets: &[usize],
    ) -> AppResult<BTreeMap<usize, ReviewedLine>> {
        let inputs: Vec<_> = transcript
            .iter()
            .filter(|line| targets.contains(&line.id))
            .cloned()
            .collect();
        if inputs.len() != targets.len() {
            return Err(invalid("全文校对字幕编号不一致"));
        }
        let glossary: Vec<_> = settings
            .preferences
            .glossary
            .iter()
            .filter(|term| inputs.iter().any(|line| term.matches(&line.original)))
            .collect();
        let mut instruction = settings.preferences.translation_rules();
        instruction.push_str("\n本次校对并翻译。chinese_reference 为可信中文参考，不得反译成所谓真实识别。original 可保守纠正近音误字，但必须同时给出 phoneticReason 发音关系与 contextReason 语境依据，不可只凭题材。无法确认时 original 原样返回，uncertain=true，note 说明疑点。locked=true 的原文原样返回。parts 按 original 的句末标点或日文空格划分意群，source 按顺序逐字覆盖 original 全部字符，保留分隔符，每个意群分别给出 translation，不合并意群。不得移动相邻内容。仅返回 targets 对应 JSON：{\"subtitles\":[{\"id\":编号,\"original\":\"原文或候选修正\",\"parts\":[{\"source\":\"原文意群\",\"translation\":\"对应译文\"}],\"uncertain\":false,\"note\":\"疑点或空串\",\"phoneticReason\":\"发音依据或空串\",\"contextReason\":\"语境依据或空串\"}]}。scope 表示实际提供的全文或分块范围，分块之外内容未知，不推测缺失上下文。");
        let limit = if settings.translation_provider == "ollama" {
            32768usize
        } else {
            120000usize
        };
        let overhead = instruction.len()
            + serde_json::json!({"preferences":settings.deepseek_prompt,"glossary":glossary,"targets":targets}).to_string().len()
            + settings.translation_options.max_tokens as usize
            + 2048;
        let window = context_window(transcript, targets, limit.saturating_sub(overhead))?;
        let scope = serde_json::json!({"mode":if window.len()==transcript.len(){"full"}else{"chunk"},"firstId":window[0].id,"lastId":window[window.len()-1].id,"count":window.len(),"total":transcript.len()});
        let user=serde_json::json!({"preferences":settings.deepseek_prompt,"transcript":window,"targets":targets,"glossary":glossary,"scope":scope}).to_string();
        let messages = serde_json::json!([{"role":"system","content":instruction},{"role":"user","content":user}]);
        let schema = serde_json::json!({"type":"object","additionalProperties":false,"required":["subtitles"],"properties":{"subtitles":{"type":"array","items":{"type":"object","additionalProperties":false,"required":["id","original","parts","uncertain","note","phoneticReason","contextReason"],"properties":{"id":{"type":"integer","enum":targets},"original":{"type":"string"},"parts":{"type":"array","items":{"type":"object","additionalProperties":false,"required":["source","translation"],"properties":{"source":{"type":"string"},"translation":{"type":"string"}}}},"uncertain":{"type":"boolean"},"note":{"type":"string"},"phoneticReason":{"type":"string"},"contextReason":{"type":"string"}}}}}});
        let client = Self::client(settings.translation_provider == "ollama")?;
        let value: serde_json::Value = if settings.translation_provider == "ollama" {
            Self::checked(client.post(format!("{}/api/chat",settings.ollama_url)).timeout(Duration::from_secs(300))
                .json(&serde_json::json!({"model":settings.ollama_model,"stream":false,"think":settings.translation_options.ollama_think(),"format":schema,"messages":messages,"options":{"num_ctx":32768,"num_predict":settings.translation_options.max_tokens,"temperature":settings.translation_options.temperature}}))
                .send().await.map_err(network_error)?).await?.json().await.map_err(network_error)?
        } else {
            let key = self
                .key()?
                .ok_or_else(|| invalid("请先保存 DeepSeek API Key"))?;
            Self::checked(
                client
                    .post("https://api.deepseek.com/chat/completions")
                    .timeout(Duration::from_secs(300))
                    .bearer_auth(key)
                    .json(
                        &settings
                            .translation_options
                            .deepseek_body(&settings.deepseek_model, messages),
                    )
                    .send()
                    .await
                    .map_err(network_error)?,
            )
            .await?
            .json()
            .await
            .map_err(network_error)?
        };
        let content = if settings.translation_provider == "ollama" {
            value["message"]["content"].as_str()
        } else {
            value["choices"][0]["message"]["content"].as_str()
        }
        .ok_or_else(|| invalid("校对未返回正文"))?;
        // Even a length-limited response can contain complete valid entries; parse each ID.
        let mut reviewed = parse_review(content, &inputs, settings)?;
        reviewed.retain(|_, line| {
            !super::local_translation::is_untranslated_copy(
                &line.original,
                &line.translation,
                settings,
            )
        });
        for line in reviewed.values_mut() {
            line.trace.context_first_id = window[0].id;
            line.trace.context_last_id = window[window.len() - 1].id;
            line.trace.context_count = window.len();
            line.trace.total_count = transcript.len();
        }
        Ok(reviewed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn parse_review(
        content: &str,
        inputs: &[ReviewInput],
    ) -> AppResult<BTreeMap<usize, ReviewedLine>> {
        super::parse_review(content, inputs, &RecognitionSettings::default())
    }
    #[tokio::test]
    async fn full_review_retries_only_missing_ids_and_records_chunk_scope() {
        use wiremock::{
            matchers::{method, path},
            Mock, MockServer, ResponseTemplate,
        };
        let temp = tempfile::tempdir().unwrap();
        let db =
            Database::open_and_migrate(&temp.path().join("d"), &temp.path().join("c")).unwrap();
        let svc = RecognitionService::new(db.clone(), db.data_dir.clone(), "test.review.retry");
        let server = MockServer::start().await;
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let count = calls.clone();
        Mock::given(method("POST")).and(path("/api/chat")).respond_with(move |request:&wiremock::Request| {
            let body:serde_json::Value=request.body_json().unwrap();
            let input:serde_json::Value=serde_json::from_str(body["messages"][1]["content"].as_str().unwrap()).unwrap();
            assert_eq!(input["preferences"],"保留称呼");
            assert_eq!(input["scope"]["mode"],"chunk");
            let n=count.fetch_add(1,std::sync::atomic::Ordering::SeqCst);
            let id=if n==0 { assert_eq!(input["targets"],serde_json::json!([7,9])); 9 } else {assert_eq!(input["targets"],serde_json::json!([7]));7};
            let output=serde_json::json!({"subtitles":[{"id":id,"original":"塩を入れる","parts":[{"source":"塩を入れる","translation":"放盐"}],"uncertain":true,"note":"请听音核对"}]});
            ResponseTemplate::new(200).set_body_json(serde_json::json!({"message":{"content":output.to_string()}}))
        }).expect(2).mount(&server).await;
        let settings = RecognitionSettings {
            ollama_url: server.uri(),
            deepseek_prompt: "保留称呼".into(),
            ..Default::default()
        };
        let mut transcript = vec![
            ReviewInput {
                id: 7,
                original: "塩を入れる".into(),
                locked: false,
                chinese_reference: None,
            },
            ReviewInput {
                id: 9,
                original: "塩を入れる".into(),
                locked: false,
                chinese_reference: None,
            },
        ];
        transcript.extend((10..100).map(|id| ReviewInput {
            id,
            original: "無関係".repeat(200),
            locked: false,
            chinese_reference: None,
        }));
        let result = svc
            .review_full_text(&settings, &transcript, &[7, 9])
            .await
            .unwrap();
        assert_eq!(result.len(), 2);
        assert!(result[&7].trace.context_count < result[&7].trace.total_count);
        assert_eq!(result[&7].original, "塩を入れる");
    }

    #[test]
    fn review_requires_both_reasons_preserves_uncertainty_and_protects_trusted_chinese() {
        let cue = crate::domain::SubtitleCueDto {
            index: 0,
            start_ms: 0,
            end_ms: 1000,
            text: "地首".into(),
        };
        let mut doc = SubtitleDocument::new(
            "v".into(),
            "sensevoice:test".into(),
            vec![cue.clone()],
            true,
        );
        let input = review_input(&doc);
        let unsupported = r#"{"subtitles":[{"id":0,"original":"乳首","parts":[{"source":"乳首","translation":"乳头"}]}]}"#;
        assert!(parse_review(unsupported, &input).unwrap().is_empty());
        let uncertain = r#"{"subtitles":[{"id":0,"original":"地首","uncertain":true,"note":"候选近音，需听音确认","parts":[{"source":"地首","translation":"地首"}]}]}"#;
        doc.reference_subtitles = vec![crate::domain::SubtitleCueDto {
            text: "可信中文字幕".into(),
            ..cue
        }];
        apply_review(&mut doc, parse_review(uncertain, &input).unwrap());
        assert_eq!(doc.original_text(&doc.original[0]), "地首");
        assert_eq!(doc.translation_text(0), Some("可信中文字幕"));
        assert!(!doc.translations.contains_key(&0));
        assert_eq!(doc.review_pending_count(), 1);
        let unchanged = r#"{"subtitles":[{"id":0,"original":"地首","parts":[{"source":"地首","translation":"地首"}]}]}"#;
        apply_review(&mut doc, parse_review(unchanged, &input).unwrap());
        assert_eq!(
            doc.review_pending_count(),
            1,
            "A model cannot acknowledge its own uncertain candidate"
        );
        doc.translation_corrections.insert(0, "人工译文".into());
        apply_review(&mut doc, parse_review(uncertain, &input).unwrap());
        assert_eq!(doc.translation_text(0), Some("人工译文"));
        let roundtrip: SubtitleDocument =
            serde_json::from_str(&serde_json::to_string(&doc).unwrap()).unwrap();
        assert_eq!(roundtrip.review_pending_count(), 1);
        roundtrip.validate().unwrap();
    }
    #[tokio::test]
    async fn t1513_full_review_sends_entire_context_and_shared_prompt_to_ollama() {
        use wiremock::{
            matchers::{method, path},
            Mock, MockServer, ResponseTemplate,
        };
        let temp = tempfile::tempdir().unwrap();
        let db = Database::open_and_migrate(&temp.path().join("data"), &temp.path().join("cache"))
            .unwrap();
        let svc = RecognitionService::new(db.clone(), db.data_dir.clone(), "test.full.review");
        let server = MockServer::start().await;
        Mock::given(method("POST")).and(path("/api/chat")).respond_with(|request:&wiremock::Request| {
            let body:serde_json::Value=serde_json::from_slice(&request.body).unwrap();
            assert!(body["messages"][1]["content"].as_str().unwrap().contains("保留称呼"));
            assert_eq!(body["options"]["num_ctx"],32768);
            let content:serde_json::Value=serde_json::from_str(body["messages"][1]["content"].as_str().unwrap()).unwrap();
            assert_eq!(content["transcript"].as_array().unwrap().len(),2);
            assert_eq!(content["targets"],serde_json::json!([8]));
            ResponseTemplate::new(200).set_body_json(serde_json::json!({"done":true,"done_reason":"stop","message":{"content":r#"{"subtitles":[{"id":8,"original":"こんばんは","parts":[{"source":"こんばんは","translation":"晚上好"}]}]}"#}}))
        }).expect(1).mount(&server).await;
        let settings = RecognitionSettings {
            ollama_url: server.uri(),
            deepseek_prompt: "保留称呼".into(),
            full_text_review: true,
            ..Default::default()
        };
        let transcript = vec![
            ReviewInput {
                id: 8,
                original: "こんばんは".into(),
                locked: false,
                chinese_reference: None,
            },
            ReviewInput {
                id: 9,
                original: "おやすみ".into(),
                locked: false,
                chinese_reference: None,
            },
        ];
        let result = svc
            .review_full_text(&settings, &transcript, &[8])
            .await
            .unwrap();
        assert_eq!(result[&8].translation, "晚上好");
        let mut too_long = transcript;
        too_long[1].original = "あ".repeat(40_000);
        let window = context_window(&too_long, &[8], 8000).unwrap();
        assert_eq!(window.len(), 1);
        assert!(context_window(&too_long, &[9], 8000).is_err());
    }
    #[test]
    fn t1513_full_review_preserves_raw_timing_and_human_edits_and_rejects_missing_ids() {
        let mut doc = SubtitleDocument::new(
            "v".into(),
            "sensevoice:test".into(),
            vec![crate::domain::SubtitleCueDto {
                index: 8,
                start_ms: 10,
                end_ms: 800,
                text: "この距ョリ".into(),
            }],
            true,
        );
        let before = doc.original.clone();
        let input = review_input(&doc);
        assert!(parse_review(r#"{"subtitles":[]}"#, &input)
            .unwrap()
            .is_empty());
        assert!(parse_review(r#"{"subtitles":[{"id":9,"original":"距離","parts":[{"source":"距離","translation":"距离"}]}]}"#,&input).unwrap().is_empty());
        let valid = r#"{"subtitles":[{"id":8,"original":"この距離","phoneticReason":"距ョリ与距離读音接近","contextReason":"距离的表达","parts":[{"source":"この距離","translation":"这个距离"}]}]}"#;
        apply_review(&mut doc, parse_review(valid, &input).unwrap());
        assert_eq!(doc.original, before);
        assert_eq!(doc.rendered()[0].text, "この距離\n这个距离");
        doc.original_corrections.insert(8, "人工原文".into());
        assert!(parse_review(valid, &review_input(&doc)).unwrap().is_empty());
        doc.validate().unwrap();
        let restored: SubtitleDocument =
            serde_json::from_str(&serde_json::to_string(&doc).unwrap()).unwrap();
        assert_eq!(restored.model_corrections, doc.model_corrections);
    }
}
