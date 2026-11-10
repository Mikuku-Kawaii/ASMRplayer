//! Contextual DeepSeek batches with auditable source coverage and per-cue retry.
use super::*;
use std::collections::BTreeMap;

impl RecognitionService {
    pub(crate) async fn translate_deepseek_batch(
        &self,
        settings: &RecognitionSettings,
        lines: &[TranslationLine],
        context: &str,
    ) -> AppResult<BTreeMap<usize, String>> {
        let key = self
            .key()?
            .ok_or_else(|| invalid("请先保存 DeepSeek API Key"))?;
        self.deepseek_batch_at(
            settings,
            lines,
            context,
            "https://api.deepseek.com/chat/completions",
            &key,
        )
        .await
    }
    async fn deepseek_batch_at(
        &self,
        settings: &RecognitionSettings,
        lines: &[TranslationLine],
        context: &str,
        endpoint: &str,
        key: &str,
    ) -> AppResult<BTreeMap<usize, String>> {
        let _model_use = self
            .model_access
            .try_read()
            .map_err(|_| invalid("正在删除模型，请稍后翻译"))?;
        let settings = validate_settings(settings.clone())?;
        if settings.translation_provider != "deepseek"
            || settings.deepseek_model.is_empty()
            || lines.is_empty()
            || lines.len() > LOCAL_BATCH_SIZE
        {
            return Err(invalid("DeepSeek 模型或待翻译句子无效"));
        }
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(120))
            .build()
            .map_err(network_error)?;
        let mut accepted = BTreeMap::new();
        let mut last_error = None;
        // One contextual batch, followed by one focused attempt for each failed cue.
        for attempt in 0..2 {
            let pending: Vec<_> = lines
                .iter()
                .filter(|l| !accepted.contains_key(&l.id))
                .cloned()
                .collect();
            let width = if attempt == 0 { LOCAL_BATCH_SIZE } else { 1 };
            for group in pending.chunks(width) {
                let mut instruction = settings.preferences.translation_rules();
                instruction.push_str("\n阅读全部 sentences 与 context 后翻译。按输入 id 分别输出，每句内部按原文分句/意群列 parts；不能只译末尾，不能把邻句内容挪到本句。source 必须按顺序逐字覆盖该 id 的全部原文（包括重复、助词和拟声词），不可改写、漏字或重排。每个意群都给出对应中文，承接片段保持未完语气，不能将词尾猜成人名；疑似识别错误也不得凭空编造。输出前逐段核对否定、动作、称呼、转折与语气是否完整。仅返回 JSON：{\"translations\":[{\"id\":编号,\"parts\":[{\"source\":\"原文意群\",\"translation\":\"对应译文\"}]}]}。不要输出其他字段。");
                if attempt > 0 {
                    instruction.push_str(
                        "\n上一轮该句未通过完整性或语言检查；本次补齐全部意群，仍仅返回当前 id。",
                    );
                }
                instruction.push_str("sentences 中给出的 sourceParts 是完整覆盖原文的分句；每个 sourcePart 必须分别返回一个 part，保持原样，不得合并或漏掉。");
                let sentences: Vec<_> = group.iter().map(|line| serde_json::json!({"id":line.id,"text":line.text,"sourceParts":source_parts(&line.text)})).collect();
                let glossary: Vec<_> = settings
                    .preferences
                    .glossary
                    .iter()
                    .filter(|entry| group.iter().any(|line| entry.matches(&line.text)))
                    .collect();
                let messages = serde_json::json!([
                    {"role":"system","content":instruction},
                    {"role":"user","content":serde_json::json!({"preferences":settings.deepseek_prompt,"context":context,"sentences":sentences,"glossary":glossary}).to_string()}
                ]);
                let response = async {
                    let value: serde_json::Value = Self::checked(
                        client
                            .post(endpoint)
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
                    .map_err(network_error)?;
                    Ok::<_, crate::domain::AppError>(value)
                }
                .await;
                let value = match response {
                    Ok(value) => value,
                    Err(err) => {
                        last_error = Some(err);
                        continue;
                    }
                };
                accepted.extend(covered_translations(
                    value["choices"][0]["message"]["content"]
                        .as_str()
                        .unwrap_or(""),
                    group,
                    &settings,
                ));
            }
        }
        if accepted.is_empty() {
            if let Some(err) = last_error {
                return Err(err);
            }
        }
        Ok(accepted)
    }
}

pub(super) fn source_parts(text: &str) -> Vec<&str> {
    let japanese = text.chars().any(|c| matches!(c, '\u{3040}'..='\u{30ff}'));
    let mut parts = Vec::new();
    let mut start = 0;
    for (i, c) in text.char_indices() {
        if "。！？!?；;\n".contains(c) || (japanese && c.is_whitespace()) {
            let end = i + c.len_utf8();
            if !text[start..end].trim().is_empty() {
                parts.push(&text[start..end]);
                start = end;
            }
        }
    }
    if start < text.len() {
        if text[start..].trim().is_empty() {
            if let Some(last) = parts.last_mut() {
                let offset = text.len() - text[start..].len() - last.len();
                *last = &text[offset..];
            }
        } else {
            parts.push(&text[start..]);
        }
    }
    parts
}

pub(super) fn covered_translations(
    content: &str,
    expected: &[TranslationLine],
    settings: &RecognitionSettings,
) -> BTreeMap<usize, String> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Response {
        translations: Vec<serde_json::Value>,
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Line {
        id: usize,
        parts: Vec<Part>,
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Part {
        source: String,
        translation: String,
    }
    let mut out = BTreeMap::new();
    let Ok(response) = serde_json::from_str::<Response>(content) else {
        return out;
    };
    for input in expected {
        let matching: Vec<_> = response
            .translations
            .iter()
            .filter(|l| l["id"].as_u64() == Some(input.id as u64))
            .collect();
        if matching.len() != 1 {
            continue;
        }
        let Ok(line) = serde_json::from_value::<Line>(matching[0].clone()) else {
            continue;
        };
        if line.id != input.id {
            continue;
        }
        let parts = &line.parts;
        let source: String = parts.iter().map(|p| p.source.as_str()).collect();
        // Exact coverage also catches repeated/reordered segments. Spaces are kept.
        let expected_parts = source_parts(&input.text);
        if source != input.text
            || parts.len() != expected_parts.len()
            || parts
                .iter()
                .zip(&expected_parts)
                .any(|(p, expected)| p.source != *expected)
            || parts.is_empty()
            || parts.iter().any(|p| {
                p.source.is_empty()
                    || super::local_translation::is_untranslated_copy(
                        &p.source,
                        &p.translation,
                        settings,
                    )
                    || super::local_translation::fabricated_fragment_quote(&p.source, &p.translation, settings)
                    || validate_translation_text(&p.translation).is_err()
                    || p.translation.chars().all(|c| {
                        c.is_whitespace() || super::super::speech_engine::is_punctuation(c)
                    })
            })
        {
            continue;
        }
        let translation = parts
            .iter()
            .map(|p| p.translation.trim())
            .collect::<Vec<_>>()
            .join(" ");
        if let Ok(text) = apply_translation_punctuation(
            translation,
            &settings.preferences.translation_punctuation,
        ) {
            out.insert(input.id, text);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn coverage_keeps_negation_and_all_clauses_and_isolates_invalid_rows() {
        let settings = RecognitionSettings::default();
        let lines = [
            TranslationLine {
                id: 7,
                text: "行かない。 まだ話したい".into(),
            },
            TranslationLine {
                id: 9,
                text: "ジュルジュル".into(),
            },
        ];
        let good = serde_json::json!({"translations":[
            {"id":7,"parts":[{"source":"行かない。","translation":"不去。"},{"source":" まだ話したい","translation":"还想聊一会儿"}]},
            {"id":9,"parts":[{"source":"ジュルジュル","translation":"啾噜啾噜"}]}
        ]});
        let translated = covered_translations(&good.to_string(), &lines, &settings);
        assert_eq!(translated[&7], "不去。 还想聊一会儿");
        assert_eq!(translated[&9], "啾噜啾噜");
        let mut missing = good.clone();
        missing["translations"][0]["parts"]
            .as_array_mut()
            .unwrap()
            .pop();
        assert_eq!(
            covered_translations(&missing.to_string(), &lines, &settings),
            BTreeMap::from([(9, "啾噜啾噜".into())])
        );
        let mut duplicate = good.clone();
        duplicate["translations"]
            .as_array_mut()
            .unwrap()
            .push(good["translations"][0].clone());
        assert_eq!(
            covered_translations(&duplicate.to_string(), &lines, &settings)
                .keys()
                .copied()
                .collect::<Vec<_>>(),
            vec![9]
        );
        let mut invalid = good;
        invalid["translations"][0]["time"] = 5.into();
        assert_eq!(
            covered_translations(&invalid.to_string(), &lines, &settings).len(),
            1
        );
    }
    #[test]
    fn t1513_deepseek_coverage_rejects_missing_clauses_duplicates_and_untranslated_parts() {
        let settings = RecognitionSettings::default();
        let lines = [TranslationLine {
            id: 7,
            text: "舐めてあげる それじゃあ君の".into(),
        }];
        let valid = serde_json::json!({"translations":[{"id":7,"parts":[
            {"source":"舐めてあげる ","translation":"帮你舔一舔"},
            {"source":"それじゃあ君の","translation":"那么，你的……"}]}]});
        assert_eq!(
            covered_translations(&valid.to_string(), &lines, &settings).len(),
            1
        );
        for field in ["source", "translation"] {
            let mut bad = valid.clone();
            bad["translations"][0]["parts"][1][field] = "".into();
            assert!(covered_translations(&bad.to_string(), &lines, &settings).is_empty());
        }
        let mut kana = valid.clone();
        kana["translations"][0]["parts"][1]["translation"] = "君の".into();
        assert!(covered_translations(&kana.to_string(), &lines, &settings).is_empty());
        let merged = serde_json::json!({"translations":[{"id":7,"parts":[{"source":lines[0].text,"translation":"舔着你的"}]}]});
        assert!(covered_translations(&merged.to_string(), &lines, &settings).is_empty());
        assert_eq!(source_parts("おやすみ  ").concat(), "おやすみ  ");
        assert_eq!(source_parts("Good night"), ["Good night"]);
    }
    #[tokio::test]
    async fn t1513_deepseek_retries_only_failed_ids_with_global_prompt_and_neighbours() {
        use wiremock::{
            matchers::{method, path},
            Mock, MockServer, ResponseTemplate,
        };
        let temp = tempfile::tempdir().unwrap();
        let db = Database::open_and_migrate(&temp.path().join("data"), &temp.path().join("cache"))
            .unwrap();
        let svc =
            RecognitionService::new(db.clone(), db.data_dir.clone(), "test.deepseek.coverage");
        let server = MockServer::start().await;
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let count = calls.clone();
        Mock::given(method("POST")).and(path("/chat")).respond_with(move |request: &wiremock::Request| {
            let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
            assert!(body["messages"][1]["content"].as_str().unwrap().contains("称呼保留原文"));
            let payload: serde_json::Value = serde_json::from_str(body["messages"][1]["content"].as_str().unwrap()).unwrap();
            assert_eq!(payload["context"], "前后句上下文");
            assert_eq!(payload["glossary"], serde_json::json!([]));
            let attempt = count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let output = if attempt == 0 {
                assert_eq!(payload["sentences"].as_array().unwrap().len(), 2);
                serde_json::json!({"translations":[{"id":9,"parts":[{"source":"こんばんは","translation":"晚上好"}]}]})
            } else {
                assert_eq!(payload["sentences"].as_array().unwrap().len(), 1);
                assert_eq!(payload["sentences"][0]["id"], 7);
                serde_json::json!({"translations":[{"id":7,"parts":[{"source":"おやすみ","translation":"晚安"}]}]})
            };
            ResponseTemplate::new(200).set_body_json(serde_json::json!({"choices":[{"finish_reason":"stop","message":{"content":output.to_string()}}]}))
        }).expect(2).mount(&server).await;
        let mut settings = RecognitionSettings {
            translation_provider: "deepseek".into(),
            deepseek_model: "test".into(),
            deepseek_prompt: "称呼保留原文".into(),
            ..Default::default()
        };
        settings
            .preferences
            .glossary
            .push(super::super::super::subtitle_preferences::Term {
                source: "無関係".into(),
                target: "无关条目".into(),
                ..Default::default()
            });
        let lines = [
            TranslationLine {
                id: 7,
                text: "おやすみ".into(),
            },
            TranslationLine {
                id: 9,
                text: "こんばんは".into(),
            },
        ];
        let translated = svc
            .deepseek_batch_at(
                &settings,
                &lines,
                "前后句上下文",
                &format!("{}/chat", server.uri()),
                "test-only",
            )
            .await
            .unwrap();
        assert_eq!(
            translated,
            BTreeMap::from([(7, "晚安".into()), (9, "晚上好".into())])
        );
        svc.save_settings(settings.clone()).unwrap();
        assert_eq!(svc.settings().unwrap().deepseek_prompt, "称呼保留原文");
        let mut invalid_settings = settings;
        invalid_settings.deepseek_prompt = "字".repeat(4001);
        assert!(svc.save_settings(invalid_settings).is_err());
    }
}
