use super::subtitle_tasks::SubtitleTaskService;
use crate::{
    db::{subtitles::SubtitleDocument, Database},
    domain::{AppError, AppResult, ErrorCode},
};
use serde::{Deserialize, Serialize};
use std::{fs::OpenOptions, io::Write, path::Path};

fn invalid(message: &str) -> AppError {
    AppError::new(ErrorCode::InvalidInput, message)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditableCue {
    index: usize,
    start_ms: i64,
    end_ms: i64,
    original: String,
    translation: String,
    edited: bool,
    raw_original: String,
    model_original: Option<String>,
    review: Option<crate::db::subtitles::ModelReviewRecord>,
    status: String,
    audio_note: Option<String>,
    audio_comparison: Option<String>,
    can_exclude_non_dialogue: bool,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditableDocument {
    version_id: String,
    source_key: String,
    revision: String,
    recognition_complete: bool,
    cues: Vec<EditableCue>,
}
impl From<&SubtitleDocument> for EditableDocument {
    fn from(doc: &SubtitleDocument) -> Self {
        Self {
            version_id: doc.version_id.clone(),
            source_key: doc.source_key.clone(),
            revision: doc.revision(),
            recognition_complete: doc.recognition_complete,
            cues: doc
                .original
                .iter()
                .map(|cue| EditableCue {
                    index: cue.index,
                    start_ms: cue.start_ms,
                    end_ms: cue.end_ms,
                    original: doc.original_text(cue).into(),
                    translation: doc.translation_text(cue.index).unwrap_or("").into(),
                    edited: doc.original_corrections.contains_key(&cue.index)
                        || doc.model_corrections.contains_key(&cue.index)
                        || doc.translation_corrections.contains_key(&cue.index),
                    raw_original: doc
                        .asr_records
                        .get(&cue.index)
                        .and_then(|r| r.attempts.first())
                        .map(|a| a.result.text.clone())
                        .unwrap_or_else(|| cue.text.clone()),
                    model_original: doc.model_corrections.get(&cue.index).cloned(),
                    review: doc.model_reviews.get(&cue.index).cloned(),
                    status: doc.cue_status(cue.index).into(),
                    can_exclude_non_dialogue: doc.source_key.starts_with("sensevoice:") && doc.trusted_reference(cue.index).is_none(),
                    audio_note: doc
                        .asr_records
                        .get(&cue.index)
                        .filter(|r| !r.human_confirmed)
                        .map(|r| r.note.clone()),
                    audio_comparison: doc
                        .asr_records
                        .get(&cue.index)
                        .and_then(|r| {
                            if let Some(review) = &r.dialogue_review.as_ref().filter(|review| !review.observations.is_empty()) {
                                return Some(review.observations.iter().map(|o| format!("{} · {}–{} ms{}：{}",
                                    o.model, o.start_ms, o.end_ms, if o.context { "（含上下文）" } else { "" },
                                    if o.result.text.trim().is_empty() { "未返回文字" } else { &o.result.text }))
                                    .collect::<Vec<_>>().join("\n"));
                            }
                            r.attempts.get(1).map(|a| a.result.text.clone())
                        }),
                })
                .collect(),
        }
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubtitleEdit {
    pub version_id: String,
    pub source_key: String,
    pub revision: String,
    pub index: usize,
    pub original: String,
    pub translation: String,
    pub reset: bool,
    #[serde(default)]
    pub undo_model: bool,
    #[serde(default)]
    pub exclude_non_dialogue: bool,
}

impl SubtitleTaskService {
    pub fn import_local_subtitle(&self, version: &str, path: &Path) -> AppResult<()> {
        self.with_idle(|| {
            let (generation, _, expected, _) = self.playback.subtitle_context();
            let path = path.canonicalize()?;
            let extension = path.extension().and_then(|ext| ext.to_str()).unwrap_or("").to_ascii_lowercase();
            if !path.is_file() || !["lrc", "srt", "vtt"].contains(&extension.as_str()) {
                return Err(invalid("请选择 LRC、SRT 或 VTT 字幕文件"));
            }
            let cues = super::library::parse_subtitle_bytes(&std::fs::read(&path)?, 0)?;
            if cues.is_empty() { return Err(invalid("字幕文件没有可用的时间轴台词")); }
            let work = self.subtitle_work_context(Some(version))?.work_id;
            let id = format!("manual-subtitle:{version}");
            let title = path.file_name().unwrap_or_default().to_string_lossy();
            self.db.with_conn(|conn| {
                conn.execute("INSERT INTO attachments(id,work_id,version_id,attachment_type,title,absolute_path,subtitle_offset_ms,encoding,availability,created_at)
                    VALUES(?1,?2,?3,'subtitle',?4,?5,0,'UTF-8','available',datetime('now'))
                    ON CONFLICT(id) DO UPDATE SET title=excluded.title,absolute_path=excluded.absolute_path,subtitle_offset_ms=0,availability='available'",
                    rusqlite::params![id,work,version,title,path.to_string_lossy()])?;
                Ok(())
            })?;
            self.db.set_setting(&format!("subtitle_attachment:{version}"), &id)?;
            let rendered = self.db.subtitle_overlay(version, cues);
            self.playback.publish_generated_subtitles(version, generation, &expected, rendered);
            Ok(())
        })
    }
    pub fn refresh_restored_subtitles(&self) {
        let (generation, version, expected, _) = self.playback.subtitle_context();
        if let Some(version) = version {
            if let Ok(doc) = self.document(&version) {
                self.playback.publish_generated_subtitles(
                    &version,
                    generation,
                    &expected,
                    doc.rendered(),
                );
            }
        }
    }
    pub fn editable_document(&self, version: &str) -> AppResult<EditableDocument> {
        Ok(EditableDocument::from(&self.document(version)?))
    }
    pub fn edit_document(&self, edit: SubtitleEdit) -> AppResult<EditableDocument> {
        let result = self.with_idle(|| {
            let (generation, current, expected, _) = self.playback.subtitle_context();
            let doc = self.document(&edit.version_id)?;
            if doc.source_key != edit.source_key || doc.revision() != edit.revision {
                return Err(invalid("字幕已变化，请重新打开编辑后保存"));
            }
            let doc = self.db.with_conn(|conn| {
                let mut doc = doc.clone();
                apply_edit(&mut doc, &edit)?;
                Database::write_subtitle_document(conn, &doc)?;
                Ok(doc)
            })?;
            if current.as_deref() == Some(&edit.version_id) {
                self.playback.publish_generated_subtitles(
                    &edit.version_id,
                    generation,
                    &expected,
                    doc.rendered(),
                );
            }
            Ok(EditableDocument::from(&doc))
        });
        if let Ok(doc) = &result {
            self.set_review_pending(
                &doc.version_id,
                doc.cues
                    .iter()
                    .filter(|cue| {
                        cue.status == "audioReview" || cue.status != "nonDialogue" && cue.review
                            .as_ref()
                            .is_some_and(|review| !review.note.is_empty())
                    })
                    .count(),
            );
        }
        result
    }
    pub fn export_document(
        &self,
        version: &str,
        revision: &str,
        format: SubtitleFormat,
        mode: SubtitleMode,
        path: &Path,
    ) -> AppResult<SubtitleExportResult> {
        let doc = self.document(version)?;
        if doc.revision() != revision {
            return Err(invalid("字幕已变化，请重新打开导出后重试"));
        }
        let (text, result) = render_export(&doc, format, mode)?;
        write_subtitle(path, text.as_bytes())?;
        Ok(result)
    }
}
fn apply_edit(doc: &mut SubtitleDocument, edit: &SubtitleEdit) -> AppResult<()> {
    let cue = doc
        .original
        .iter()
        .find(|c| c.index == edit.index)
        .ok_or_else(|| invalid("该字幕句已不存在，请重新打开"))?;
    if edit.exclude_non_dialogue {
        if !doc.source_key.starts_with("sensevoice:") || doc.trusted_reference(edit.index).is_some() {
            return Err(invalid("只能标记生成的识别字幕，文件自带字幕保留"));
        }
        doc.non_dialogue_cues.insert(edit.index);
        doc.translations.remove(&edit.index);
        doc.translation_failures.remove(&edit.index);
        return doc.validate();
    }
    if edit.undo_model {
        doc.model_corrections.remove(&edit.index);
        doc.model_reviews.remove(&edit.index);
        doc.translations.remove(&edit.index);
        return doc.validate();
    }
    let original = edit
        .original
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .trim()
        .to_owned();
    let translation = edit
        .translation
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .trim()
        .to_owned();
    if !edit.reset
        && (original.is_empty()
            || original.len() > 100_000
            || translation.len() > 100_000
            || original.contains('\0')
            || translation.contains('\0'))
    {
        return Err(invalid(
            "原文不能为空，单句文本不能超过 100 KB 或包含空字符",
        ));
    }
    let original_changed = edit.reset
        && (doc.original_corrections.contains_key(&edit.index)
            || doc.model_corrections.contains_key(&edit.index))
        || !edit.reset && doc.original_text(cue) != original;
    let translation_changed = doc.translation_text(edit.index).unwrap_or("") != translation;
    let had_review = doc.cue_status(edit.index) == "nonDialogue" || doc.model_reviews.contains_key(&edit.index)
        || doc
            .asr_records
            .get(&edit.index)
            .is_some_and(|r| r.pending_audio_review);
    if edit.reset {
        doc.original_corrections.remove(&edit.index);
        doc.model_corrections.remove(&edit.index);
        doc.translation_corrections.remove(&edit.index);
    } else {
        doc.non_dialogue_cues.remove(&edit.index);
        if !had_review
            && original
                == doc
                    .rule_corrections
                    .get(&edit.index)
                    .unwrap_or(&cue.text)
                    .as_str()
        {
            doc.original_corrections.remove(&edit.index);
        } else {
            doc.original_corrections.insert(edit.index, original);
        }
        if translation_changed {
            doc.translation_corrections.insert(edit.index, translation);
        }
    }
    // Explicit save acknowledges a candidate, including an unchanged uncertain ASR line.
    // Keep its provenance while the human original remains a separate authoritative layer.
    if edit.reset {
        doc.model_reviews.remove(&edit.index);
    } else if let Some(review) = doc.model_reviews.get_mut(&edit.index) {
        review.note.clear();
    }
    if original_changed {
        doc.translations.remove(&edit.index);
    }
    if let Some(record) = doc.asr_records.get_mut(&edit.index) {
        record.human_confirmed = !edit.reset;
    }
    if !edit.reset {
        doc.translation_failures.remove(&edit.index);
    }
    doc.validate()
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SubtitleFormat {
    Srt,
    Vtt,
    Txt,
}
#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SubtitleMode {
    Original,
    Translation,
    Bilingual,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubtitleExportResult {
    pub exported_cues: usize,
    pub missing_translations: usize,
    pub recognition_complete: bool,
}
fn timestamp(ms: i64, vtt: bool) -> String {
    format!(
        "{:02}:{:02}:{:02}{}{:03}",
        ms / 3_600_000,
        ms / 60_000 % 60,
        ms / 1000 % 60,
        if vtt { '.' } else { ',' },
        ms % 1000
    )
}
pub(crate) fn render_export(
    doc: &SubtitleDocument,
    format: SubtitleFormat,
    mode: SubtitleMode,
) -> AppResult<(String, SubtitleExportResult)> {
    doc.validate()?;
    let mut cues: Vec<_> = doc.original.iter().filter(|cue| !doc.dialogue_text(cue).trim().is_empty()).cloned().collect();
    cues.sort_by_key(|c| (c.start_ms, c.index));
    let mut output = if matches!(format, SubtitleFormat::Vtt) {
        "WEBVTT\n\n".to_owned()
    } else {
        String::new()
    };
    let mut count = 0;
    let missing = cues
        .iter()
        .filter(|c| {
            doc.translation_text(c.index)
                .unwrap_or("")
                .trim()
                .is_empty()
        })
        .count();
    for cue in cues {
        let original = doc.dialogue_text(&cue);
        let translated = doc.translation_text(cue.index).unwrap_or("");
        let text = match mode {
            SubtitleMode::Original => original.to_owned(),
            SubtitleMode::Translation => translated.to_owned(),
            SubtitleMode::Bilingual if translated.is_empty() || original == translated => {
                original.to_owned()
            }
            SubtitleMode::Bilingual => format!("{original}\n{translated}"),
        };
        if text.trim().is_empty() {
            continue;
        }
        count += 1;
        if matches!(format, SubtitleFormat::Txt) {
            output.push_str(&format!("[{} – {}]\n", timestamp(cue.start_ms, true), timestamp(cue.end_ms, true)));
            output.push_str(&text);
            output.push_str("\n\n");
            continue;
        }
        let text = text
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        // WebVTT text is HTML-like. Escape content so literal tags remain text.
        let text = if matches!(format, SubtitleFormat::Vtt) {
            text.replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;")
        } else {
            text
        };
        output.push_str(&format!(
            "{count}\n{} --> {}\n{text}\n\n",
            timestamp(cue.start_ms, matches!(format, SubtitleFormat::Vtt)),
            timestamp(cue.end_ms, matches!(format, SubtitleFormat::Vtt))
        ));
    }
    if count == 0 {
        return Err(invalid(
            "当前内容没有可导出的字幕句，请选择其他内容或先生成译文",
        ));
    }
    Ok((
        output,
        SubtitleExportResult {
            exported_cues: count,
            missing_translations: missing,
            recognition_complete: doc.recognition_complete,
        },
    ))
}
pub(crate) fn write_subtitle(path: &Path, bytes: &[u8]) -> AppResult<()> {
    if !path.is_absolute() || bytes.len() > 64 * 1024 * 1024 {
        return Err(invalid("请选择有效的导出路径，字幕文件不能超过 64 MB"));
    }
    let parent = path.parent().ok_or_else(|| invalid("导出路径无效"))?;
    let temp = parent.join(format!(".asmr-subtitle-{}.part", uuid::Uuid::new_v4()));
    let result = (|| -> AppResult<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temp, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result.map_err(|_| invalid("字幕导出失败，请检查目标目录、文件占用及可用空间后重试"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::SubtitleCueDto;
    fn document() -> SubtitleDocument {
        SubtitleDocument::new(
            "v".into(),
            "sensevoice:test".into(),
            vec![
                SubtitleCueDto {
                    index: 12,
                    start_ms: 1001,
                    end_ms: 2500,
                    text: "おはよう & <雨>".into(),
                },
                SubtitleCueDto {
                    index: 81,
                    start_ms: 3_661_234,
                    end_ms: 3_662_000,
                    text: "こんばんは".into(),
                },
            ],
            false,
        )
    }
    #[test]
    fn automatic_audio_filter_roundtrips_exports_and_human_restore_survives_background_save() {
        use crate::domain::asr::{DialogueReview, DialogueDecision, DialogueObservation, RecognitionEvidence};
        let mut doc = document();
        let mut record = super::super::dialogue_review::tests::record();
        let raw = doc.original.clone();
        super::super::dialogue_review::apply(&mut record, DialogueReview {
            context_ms: 2500,
            revision: super::super::dialogue_review::REVIEW_REVISION,
            decision: DialogueDecision::NonDialogue, note: "自动复核".into(),
            observations: vec![DialogueObservation { model: "sensevoice".into(), start_ms: 0, end_ms: 5000,
                context: true, elapsed_ms: 4, result: RecognitionEvidence { event: "<|Breath|>".into(), ..Default::default() } }],
        });
        let original_attempts = serde_json::to_value(&record.attempts).unwrap();
        doc.asr_records.insert(12, record);
        doc.translations.insert(12, "旧自动译文".into());
        doc.translations.insert(81, "晚上好".into());
        let mut doc: SubtitleDocument = serde_json::from_str(&serde_json::to_string(&doc).unwrap()).unwrap();
        assert_eq!(doc.cue_status(12), "nonDialogue");
        assert!(!doc.can_translate(12));
        assert_eq!(doc.review_pending_count(), 0);
        assert_eq!(doc.original, raw);
        assert_eq!(doc.rendered().len(), 1);
        let edit_view = EditableDocument::from(&doc);
        assert_eq!(edit_view.cues[0].status, "nonDialogue");
        assert!(edit_view.cues[0].audio_comparison.as_ref().unwrap().contains("sensevoice · 0–5000 ms"));
        for mode in [SubtitleMode::Original, SubtitleMode::Translation, SubtitleMode::Bilingual] {
            let (text, _) = render_export(&doc, SubtitleFormat::Txt, mode).unwrap();
            assert!(!text.contains("旧自动译文"));
        }
        // Manual translations and trusted references independently protect their cue.
        doc.translation_corrections.insert(12, "人工译文".into());
        assert_eq!(doc.translation_text(12), Some("人工译文"));
        doc.translation_corrections.clear();
        let stale = doc.clone();
        let edit = SubtitleEdit { version_id: doc.version_id.clone(), source_key: doc.source_key.clone(),
            revision: doc.revision(), index: 12, original: raw[0].text.clone(), translation: String::new(),
            reset: false, undo_model: false, exclude_non_dialogue: false };
        apply_edit(&mut doc, &edit).unwrap();
        assert!(doc.can_translate(12));
        assert_eq!(doc.rendered().len(), 2);
        assert_eq!(serde_json::to_value(&doc.asr_records[&12].attempts).unwrap(), original_attempts);
        assert!(doc.asr_records[&12].dialogue_review.is_some());
        let root = tempfile::tempdir().unwrap();
        let db = Database::open_and_migrate(&root.path().join("data"), &root.path().join("cache")).unwrap();
        db.with_conn(|conn| {
            conn.execute_batch("INSERT INTO works(id,title,created_at,updated_at) VALUES('w','w','now','now'); INSERT INTO tracks(id,work_id,title,sort_key,created_at) VALUES('t','w','t','1','now'); INSERT INTO track_versions(id,track_id,version_label,created_at) VALUES('v','t','original','now');")?;
            Database::write_subtitle_document(conn, &doc)
        }).unwrap();
        db.save_subtitle_document(&stale).unwrap();
        let reopened = db.subtitle_document("v", &doc.source_key).unwrap().unwrap();
        assert!(reopened.asr_records[&12].human_confirmed);
        assert!(reopened.can_translate(12));
        assert_eq!(reopened.original, raw);
        assert!(super::super::dialogue_review::cached_candidates(&reopened).is_empty());
    }
    #[test]
    fn hidden_non_dialogue_is_excluded_from_exports_and_can_be_restored() {
        let mut doc=document();
        doc.original[0].text="ジュルジュル".into();
        let (text,result)=render_export(&doc,SubtitleFormat::Srt,SubtitleMode::Original).unwrap();
        assert!(!text.contains("ジュル"));
        assert!(text.contains("こんばんは"));
        assert_eq!(result.exported_cues,1);
        assert_eq!(result.missing_translations,1);
        doc.original[0].text="ごめん".into();
        doc.original[1].text="ごめん".into();
        doc.translations.insert(12,"自动译文".into());
        let mut edit=SubtitleEdit {version_id:"v".into(),source_key:doc.source_key.clone(),revision:doc.revision(),index:12,original:"ごめん".into(),translation:String::new(),reset:false,undo_model:false,exclude_non_dialogue:true};
        apply_edit(&mut doc,&edit).unwrap();
        assert_eq!(doc.rendered().len(),1);
        assert_eq!(doc.rendered()[0].index,81); // Same ordinary word elsewhere stays visible.
        assert!(!doc.translations.contains_key(&12));
        assert_eq!(doc.original[0].text,"ごめん");
        doc.source_key="external:test".into();
        assert!(apply_edit(&mut doc,&edit).is_err());
        doc.source_key=edit.source_key.clone();
        edit.exclude_non_dialogue=false;
        apply_edit(&mut doc,&edit).unwrap();
        assert!(doc.non_dialogue_cues.is_empty());
        assert_eq!(doc.rendered()[0].text,"ごめん");
        assert!(doc.original_corrections.contains_key(&12));
    }
    #[test]
    fn t1513_edit_uses_rule_layer_as_baseline_and_can_override_it_with_raw_text() {
        let mut doc = document();
        doc.rule_corrections.insert(12, "规则原文".into());
        let mut edit = SubtitleEdit {
            version_id: doc.version_id.clone(),
            source_key: doc.source_key.clone(),
            revision: doc.revision(),
            index: 12,
            original: "规则原文".into(),
            translation: "手工译文".into(),
            reset: false,
            undo_model: false,
            exclude_non_dialogue: false,
        };
        apply_edit(&mut doc, &edit).unwrap();
        assert!(!doc.original_corrections.contains_key(&12));
        edit.original = doc.original[0].text.clone();
        apply_edit(&mut doc, &edit).unwrap();
        assert_eq!(doc.original_text(&doc.original[0]), doc.original[0].text);
        assert!(doc.original_corrections.contains_key(&12));
        edit.reset = true;
        apply_edit(&mut doc, &edit).unwrap();
        assert_eq!(doc.original_text(&doc.original[0]), "规则原文");
    }
    #[test]
    fn undo_model_keeps_raw_ids_timing_human_text_and_reference() {
        let mut doc = document();
        let raw = doc.original.clone();
        doc.model_corrections.insert(12, "模型候选".into());
        doc.model_reviews.insert(
            12,
            crate::db::subtitles::ModelReviewRecord {
                note: "待听音".into(),
                ..Default::default()
            },
        );
        doc.original_corrections.insert(12, "人工原文".into());
        doc.translation_corrections.insert(12, "人工译文".into());
        let edit = SubtitleEdit {
            version_id: doc.version_id.clone(),
            source_key: doc.source_key.clone(),
            revision: doc.revision(),
            index: 12,
            original: String::new(),
            translation: String::new(),
            reset: false,
            undo_model: true,
            exclude_non_dialogue: false,
        };
        apply_edit(&mut doc, &edit).unwrap();
        assert_eq!(doc.original, raw);
        assert_eq!(doc.original_text(&doc.original[0]), "人工原文");
        assert_eq!(doc.translation_text(12), Some("人工译文"));
        assert!(doc.model_corrections.is_empty());
        assert!(doc.model_reviews.is_empty());
    }
    #[test]
    fn confirming_unchanged_uncertain_original_locks_it_and_keeps_review_provenance() {
        let mut doc = document();
        doc.model_reviews.insert(
            12,
            crate::db::subtitles::ModelReviewRecord {
                note: "待听音".into(),
                context_count: 2,
                total_count: 2,
                ..Default::default()
            },
        );
        let original = doc.original[0].text.clone();
        let edit = SubtitleEdit {
            version_id: doc.version_id.clone(),
            source_key: doc.source_key.clone(),
            revision: doc.revision(),
            index: 12,
            original: original.clone(),
            translation: String::new(),
            reset: false,
            undo_model: false,
            exclude_non_dialogue: false,
        };
        apply_edit(&mut doc, &edit).unwrap();
        assert_eq!(doc.original_corrections[&12], original);
        assert_eq!(doc.review_pending_count(), 0);
        assert_eq!(doc.model_reviews[&12].context_count, 2);
    }
    #[test]
    fn t1513_edit_and_all_export_formats_preserve_ids_times_and_partial_scope() {
        let mut doc = document();
        doc.translations.insert(12, "自动译文".into());
        let edit = SubtitleEdit {
            version_id: "v".into(),
            source_key: doc.source_key.clone(),
            revision: doc.revision(),
            index: 12,
            original: "おはよう & <雨> 修正".into(),
            translation: "早上好\n下雨了".into(),
            reset: false,
            undo_model: false,
            exclude_non_dialogue: false,
        };
        apply_edit(&mut doc, &edit).unwrap();
        assert!(!doc.translations.contains_key(&12));
        assert_eq!(doc.original[0].text, "おはよう & <雨>");
        assert_eq!(doc.rendered()[0].index, 12);
        for format in [
            SubtitleFormat::Srt,
            SubtitleFormat::Vtt,
            SubtitleFormat::Txt,
        ] {
            for mode in [
                SubtitleMode::Original,
                SubtitleMode::Translation,
                SubtitleMode::Bilingual,
            ] {
                let (text, result) = render_export(&doc, format, mode).unwrap();
                assert!(!result.recognition_complete);
                assert_eq!(result.missing_translations, 1);
                assert_eq!(
                    result.exported_cues,
                    if matches!(mode, SubtitleMode::Translation) {
                        1
                    } else {
                        2
                    }
                );
                if !matches!(format, SubtitleFormat::Txt) {
                    let parsed =
                        crate::services::library::parse_subtitle_bytes(text.as_bytes(), 0).unwrap();
                    assert_eq!(parsed.len(), result.exported_cues);
                    assert_eq!((parsed[0].start_ms, parsed[0].end_ms), (1001, 2500));
                    assert_eq!(
                        parsed[0].text,
                        match mode {
                            SubtitleMode::Original => edit.original.clone(),
                            SubtitleMode::Translation => edit.translation.clone(),
                            SubtitleMode::Bilingual =>
                                format!("{}\n{}", edit.original, edit.translation),
                        }
                    );
                    if parsed.len() == 2 {
                        assert_eq!(parsed[1].start_ms, 3_661_234);
                    }
                } else {
                    assert!(text.starts_with("[00:00:01.001 – 00:00:02.500]\n"));
                    if result.exported_cues == 2 {
                        assert!(text.contains("[01:01:01.234 – 01:01:02.000]\n"));
                    }
                }
            }
        }
        apply_edit(
            &mut doc,
            &SubtitleEdit {
                reset: true,
                undo_model: false,
            exclude_non_dialogue: false,
                ..edit
            },
        )
        .unwrap();
        assert!(doc.original_corrections.is_empty() && doc.translation_corrections.is_empty());
        assert!(render_export(&doc, SubtitleFormat::Srt, SubtitleMode::Translation).is_err());
        doc.original[1].index = 12;
        assert!(render_export(&doc, SubtitleFormat::Srt, SubtitleMode::Original).is_err());
    }
    #[test]
    fn t1513_export_write_is_atomic_and_failed_destination_preserves_previous_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("字幕.srt");
        write_subtitle(&path, b"previous").unwrap();
        let lock = crate::services::storage_file::LockedFile::open(&path, true).unwrap();
        assert!(write_subtitle(&path, b"replacement").is_err());
        drop(lock);
        assert_eq!(std::fs::read(&path).unwrap(), b"previous");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
        let (text, _) =
            render_export(&document(), SubtitleFormat::Vtt, SubtitleMode::Original).unwrap();
        write_subtitle(&path, text.as_bytes()).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
        assert!(write_subtitle(&dir.path().join("missing/sub.srt"), b"data").is_err());
    }
}
