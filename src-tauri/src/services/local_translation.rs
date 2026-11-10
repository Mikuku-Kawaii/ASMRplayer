//! Bounded local translation requests. Cue IDs are preserved; timestamps never leave the host.
use super::{invalid, network_error, validate_settings, RecognitionService, RecognitionSettings};
use crate::domain::AppResult;
use serde::Serialize;
use std::{collections::BTreeMap, time::Duration};

pub(crate) const LOCAL_BATCH_SIZE: usize = 6;
pub(crate) const LOCAL_BATCH_CHARS: usize = 600;

#[derive(Clone, Debug, Serialize)]
pub(crate) struct TranslationLine {
    pub id: usize,
    pub text: String,
}

impl RecognitionService {
    pub(crate) async fn translate_local_batch(
        &self,
        settings: &RecognitionSettings,
        sentences: &[TranslationLine],
        context: &str,
    ) -> AppResult<BTreeMap<usize, String>> {
        let _model_use = self
            .model_access
            .try_read()
            .map_err(|_| invalid("正在删除模型，请稍后翻译"))?;
        let settings = validate_settings(settings.clone())?;
        if settings.translation_provider != "ollama"
            || sentences.is_empty()
            || sentences.len() > LOCAL_BATCH_SIZE
        {
            return Err(invalid("本地翻译批次无效"));
        }
        let mut accepted = BTreeMap::new();
        let mut last_error = None;
        for attempt in 0..2 {
            // Keep valid lines; retry only failed lines individually with the same context.
            let requests = if attempt == 0 {
                vec![sentences.to_vec()]
            } else {
                sentences
                    .iter()
                    .filter(|line| !accepted.contains_key(&line.id))
                    .map(|line| vec![line.clone()])
                    .collect()
            };
            for request in requests {
                let sentences = request.as_slice();
                let glossary: Vec<_> = settings
                    .preferences
                    .glossary
                    .iter()
                    .filter(|entry| sentences.iter().any(|line| entry.matches(&line.text)))
                    .collect();

                let ids: Vec<_> = sentences.iter().map(|line| line.id).collect();
                let schema = serde_json::json!({
                    "type":"object", "additionalProperties":false, "required":["translations"],
                    "properties":{"translations":{"type":"array", "minItems":ids.len(), "maxItems":ids.len(),
                        "items":{"type":"object", "additionalProperties":false, "required":["id","parts"],
                            "properties":{"id":{"type":"integer","enum":ids},"parts":{"type":"array","items":{"type":"object","additionalProperties":false,"required":["source","translation"],"properties":{"source":{"type":"string"},"translation":{"type":"string"}}}}}}}}
                });
                let mut instruction = settings.preferences.translation_batch_instruction();
                if attempt > 0 {
                    instruction.push_str("上次结果仍含未翻译的日语或格式不完整。本次只重译当前句：忠于原文，用上下文消歧，拟声词译为中文拟声词。检查 translation 是简体中文，不得返回日语原句；按原 id 返回。");
                }
                let payload: Vec<_> = sentences.iter().map(|line| serde_json::json!({"id":line.id,"text":line.text,"sourceParts":super::deepseek_translation::source_parts(&line.text)})).collect();
                // Chat only uses installed models. There is deliberately no pull/download fallback.
                let response = async {
                    let value: serde_json::Value = Self::checked(Self::client(true)?
            .post(format!("{}/api/chat", settings.ollama_url))
            .timeout(Duration::from_secs(180))
            .json(&serde_json::json!({
                "model":settings.ollama_model, "stream":false, "think":settings.translation_options.ollama_think(), "format":schema,
                "messages":[
                    {"role":"system","content":instruction},
                    {"role":"user","content":serde_json::json!({"preferences":settings.deepseek_prompt,"context":context,"sentences":payload,"glossary":glossary}).to_string()}
                ],
                "options":{"temperature":settings.translation_options.temperature,"num_predict":settings.translation_options.max_tokens}
            })).send().await.map_err(network_error)?).await?
            .json().await.map_err(network_error)?;
                    Ok::<_,crate::domain::AppError>(value)
                }.await;
                let value = match response {
                    Ok(value) => value,
                    Err(err) => {
                        last_error = Some(err);
                        continue;
                    }
                };
                accepted.extend(super::deepseek_translation::covered_translations(
                    value["message"]["content"].as_str().unwrap_or(""),
                    sentences,
                    &settings,
                ));
            }
        }
        // The task saves these immediately, continues later cues, and reports remaining IDs.
        if accepted.is_empty() {
            if let Some(err) = last_error {
                return Err(err);
            }
        }
        Ok(accepted)
    }
}

/// Reject newly invented quotations used to smuggle an untranslated fragment
/// into Chinese prose. Original quotations and explicit glossary preferences
/// remain valid; kana in a name or elsewhere is not a failure by itself.
pub(super) fn fabricated_fragment_quote(source: &str, translation: &str, settings: &RecognitionSettings) -> bool {
    for (open, close) in [('『','』'),('「','」'),('“','”'),('‘','’'),('"','"')] {
        let mut rest = translation;
        while let Some(begin) = rest.find(open) {
            rest = &rest[begin+open.len_utf8()..];
            let Some(end) = rest.find(close) else { break; };
            let quoted = rest[..end].trim();
            let kana = quoted.chars().filter(|c| matches!(c, '\u{3041}'..='\u{3096}' | '\u{30a1}'..='\u{30fa}')).count();
            if kana >= 2 && source.contains(quoted)
                && !source.contains(&format!("{open}{quoted}{close}"))
                && !settings.preferences.glossary.iter().any(|t| t.matches(source) && t.target.contains(quoted)) {
                return true;
            }
            rest = &rest[end+close.len_utf8()..];
        }
    }
    false
}

// This detects a copied Japanese sentence/fragment, not translation correctness.
// Names/quotes may legitimately contain kana; character counts cannot assess meaning.
pub(super) fn is_untranslated_copy(
    source: &str,
    translation: &str,
    settings: &RecognitionSettings,
) -> bool {
    let normalize = |text: &str| {
        text.chars()
            .filter(|c| c.is_alphanumeric())
            .collect::<String>()
    };
    let source = normalize(source);
    let target = normalize(translation);
    (source == target || target.chars().count() >= 2 && source.contains(&target))
        && target
            .chars()
            .any(|c| matches!(c, '\u{3041}'..='\u{3096}' | '\u{30a1}'..='\u{30fa}'))
        && !settings
            .preferences
            .glossary
            .iter()
            .any(|term| term.matches(&source) && normalize(&term.target) == target)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::{
        matchers::{method, path},
        Mock, MockServer, ResponseTemplate,
    };
    #[test]
    fn newly_quoted_fragments_are_not_translations_but_names_and_original_quotes_are_valid() {
        let mut settings = RecognitionSettings::default();
        assert!(fabricated_fragment_quote("けたりはしない", "不会做『けたり』这种事", &settings));
        assert!(!fabricated_fragment_quote("「けたり」と書いてある", "上面写着「けたり」", &settings));
        assert!(!fabricated_fragment_quote("美咲さんです", "是美咲さん", &settings));
        settings.preferences.glossary.push(crate::services::subtitle_preferences::Term {
            source:"アルファ".into(), target:"『アルファ』".into(), ..Default::default()
        });
        assert!(!fabricated_fragment_quote("アルファです", "是『アルファ』", &settings));
    }

    #[tokio::test]
    async fn fragment_failure_retries_only_bad_id_and_accepts_explicit_uncertainty() {
        let temp = tempfile::tempdir().unwrap();
        let db = crate::db::Database::open_and_migrate(&temp.path().join("d"), &temp.path().join("c")).unwrap();
        let service = RecognitionService::new(db.clone(), db.data_dir.clone(), "test.fragment");
        let server = MockServer::start().await;
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let count = calls.clone();
        Mock::given(method("POST")).and(path("/api/chat")).respond_with(move |request: &wiremock::Request| {
            let body: serde_json::Value = request.body_json().unwrap();
            assert!(body["messages"][0]["content"].as_str().unwrap().contains("〔原文不清〕"));
            let input: serde_json::Value = serde_json::from_str(body["messages"][1]["content"].as_str().unwrap()).unwrap();
            assert!(input["context"].as_str().unwrap().contains("audioContext"));
            let n = count.fetch_add(1,std::sync::atomic::Ordering::SeqCst);
            let output = if n == 0 {
                serde_json::json!({"translations":[
                    {"id":7,"parts":[{"source":"けたりはしない","translation":"不会做『けたり』这种事"}]},
                    {"id":9,"parts":[{"source":"また明日","translation":"明天见"}]}]})
            } else {
                assert_eq!(input["sentences"].as_array().unwrap().len(),1);
                assert_eq!(input["sentences"][0]["id"],7);
                serde_json::json!({"translations":[{"id":7,"parts":[{"source":"けたりはしない","translation":"不会〔原文不清〕"}]}]})
            };
            ResponseTemplate::new(200).set_body_json(serde_json::json!({"message":{"content":output.to_string()}}))
        }).expect(2).mount(&server).await;
        let settings = RecognitionSettings { ollama_url:server.uri(), ..Default::default() };
        let result = service.translate_local_batch(&settings, &[
            TranslationLine { id:7,text:"けたりはしない".into() },
            TranslationLine { id:9,text:"また明日".into() },
        ],"audioContext：未确认的音频候选").await.unwrap();
        assert_eq!(result[&9],"明天见");
        assert_eq!(result[&7],"不会〔原文不清〕");
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst),2);
    }

    #[tokio::test]
    async fn local_translation_keeps_valid_id_and_retries_missing_id_with_candidate_and_preferences(
    ) {
        let temp = tempfile::tempdir().unwrap();
        let db =
            crate::db::Database::open_and_migrate(&temp.path().join("d"), &temp.path().join("c"))
                .unwrap();
        let svc = RecognitionService::new(db.clone(), db.data_dir.clone(), "test.local.faithful");
        let server = MockServer::start().await;
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let count = calls.clone();
        Mock::given(method("POST")).and(path("/api/chat")).respond_with(move |request:&wiremock::Request| {
            let body:serde_json::Value=request.body_json().unwrap();
            let input:serde_json::Value=serde_json::from_str(body["messages"][1]["content"].as_str().unwrap()).unwrap();
            assert_eq!(input["preferences"],"称呼保持原文");
            assert_eq!(input["glossary"].as_array().unwrap().len(),1);
            assert_eq!(input["glossary"][0]["source"],"潮吹き");
            let n=count.fetch_add(1,std::sync::atomic::Ordering::SeqCst);
            let output=if n==0 {
                assert_eq!(input["sentences"].as_array().unwrap().len(),2);
                serde_json::json!({"translations":[{"id":9,"parts":[{"source":"ジュルジュル","translation":"啾噜啾噜"}]},{"id":99,"translation":"非法编号"}]})
            } else {
                assert_eq!(input["sentences"].as_array().unwrap().len(),1);
                assert_eq!(input["sentences"][0]["id"],7);
                serde_json::json!({"translations":[{"id":7,"parts":[{"source":"塩吹きではない","translation":"不是潮吹"}]}]})
            };
            ResponseTemplate::new(200).set_body_json(serde_json::json!({"message":{"content":output.to_string()}}))
        }).expect(2).mount(&server).await;
        let mut settings = RecognitionSettings {
            ollama_url: server.uri(),
            deepseek_prompt: "称呼保持原文".into(),
            ..Default::default()
        };
        settings.preferences.glossary = vec![super::super::super::subtitle_preferences::Term {
            source: "潮吹き".into(),
            target: "潮吹".into(),
            misrecognitions: vec!["塩吹き".into()],
            context: "相关语境候选".into(),
        }];
        let result = svc
            .translate_local_batch(
                &settings,
                &[
                    TranslationLine {
                        id: 7,
                        text: "塩吹きではない".into(),
                    },
                    TranslationLine {
                        id: 9,
                        text: "ジュルジュル".into(),
                    },
                ],
                "",
            )
            .await
            .unwrap();
        assert_eq!(result[&9], "啾噜啾噜");
        assert_eq!(result[&7], "不是潮吹");
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 2);
        assert!(!is_untranslated_copy(
            "美咲さんです",
            "是美咲さん",
            &settings
        ));
        assert!(is_untranslated_copy("おやすみ", "おやすみ。", &settings));
    }
}
