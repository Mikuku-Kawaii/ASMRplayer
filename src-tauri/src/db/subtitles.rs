use super::Database;
use crate::domain::{AppError, AppResult, ErrorCode, SubtitleCueDto};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubtitleDocument {
    pub version_id: String,
    pub source_key: String,
    pub original: Vec<SubtitleCueDto>,
    pub translations: BTreeMap<usize, String>,
    #[serde(default)]
    pub asr_records: BTreeMap<usize, crate::domain::asr::AsrRecord>,
    #[serde(default)]
    pub translation_failures: BTreeMap<usize, String>,
    pub completed_chunks: BTreeSet<usize>,
    pub recognition_complete: bool,
    pub translation_key: String,
    #[serde(default)]
    pub original_corrections: BTreeMap<usize, String>,
    #[serde(default)]
    pub model_corrections: BTreeMap<usize, String>,
    #[serde(default)]
    pub model_reviews: BTreeMap<usize, ModelReviewRecord>,
    #[serde(default)]
    pub reference_subtitles: Vec<SubtitleCueDto>,
    #[serde(default)]
    pub rule_corrections: BTreeMap<usize, String>,
    #[serde(default)]
    pub rules_suppressed: bool,
    #[serde(default)]
    pub translation_corrections: BTreeMap<usize, String>,
    #[serde(default)]
    pub non_dialogue_cues: BTreeSet<usize>,
    #[serde(default)]
    pub dialogue_filter_version: u8,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelReviewRecord {
    pub note: String,
    pub context_first_id: usize,
    pub context_last_id: usize,
    pub context_count: usize,
    pub total_count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dialogue_only_preserves_raw_ids_progress_human_and_external_text() {
        let cues: Vec<_> = ["ジュルジュル", "ごめん", "こんにちは フッフッフッ またね", "ごめん"].iter().enumerate()
            .map(|(index,text)| SubtitleCueDto { index, start_ms:index as i64*3000,end_ms:index as i64*3000+1000,text:(*text).into() }).collect();
        let mut doc=SubtitleDocument::new("v".into(),"sensevoice:test".into(),cues.clone(),true);
        doc.completed_chunks.insert(0);
        doc.non_dialogue_cues.insert(3); // User-confirmed sound at one specific occurrence only.
        doc.translations.insert(0,"啾噜".into());
        doc.translations.insert(2,"你好，呼呼，再见".into());
        doc.dialogue_filter_version=0; // Existing cache, not just newly recognized text.
        assert_eq!(doc.rendered().iter().map(|cue| (cue.index,cue.text.as_str())).collect::<Vec<_>>(),vec![(1,"ごめん"),(2,"こんにちは またね")]);
        assert_eq!(doc.cue_status(0),"nonDialogue");
        assert_eq!(doc.cue_status(3),"nonDialogue");
        assert!(!doc.can_translate(0) && !doc.can_translate(3));
        assert_eq!(doc.dialogue_count(), 2);
        assert!(doc.can_translate(1) && doc.can_translate(2));
        doc.prepare_dialogue_only();
        assert!(doc.translations.is_empty());
        doc.translations.insert(2,"你好，再见".into());
        assert_eq!(doc.rendered()[1].text,"こんにちは またね\n你好，再见");
        let reopened: SubtitleDocument=serde_json::from_str(&serde_json::to_string(&doc).unwrap()).unwrap();
        assert_eq!(reopened.original,cues);
        assert_eq!(reopened.completed_chunks,BTreeSet::from([0]));
        assert_eq!(reopened.rendered(),doc.rendered());
        doc.original_corrections.insert(0,"ジュルジュル".into());
        assert!(doc.can_translate(0)); // Explicitly confirmed speech takes precedence.
        let external=SubtitleDocument::new("v".into(),"external:test".into(),cues,true);
        assert_eq!(external.rendered().len(),4);
        assert_eq!(external.rendered()[0].text,"ジュルジュル");
    }
    #[test]
    fn t1513_documents_survive_reopen_and_audio_or_subtitle_changes_invalidate_cache() {
        let root = tempfile::tempdir().unwrap();
        let data = root.path().join("d");
        let cache = root.path().join("c");
        let file = root.path().join("synthetic.pcm");
        std::fs::write(&file, b"audio").unwrap();
        let db = Database::open_and_migrate(&data, &cache).unwrap();
        db.with_conn(|conn| { conn.execute_batch("INSERT INTO works(id,title,created_at,updated_at) VALUES('w','w','now','now'); INSERT INTO tracks(id,work_id,title,sort_key,created_at) VALUES('t','w','t','1','now'); INSERT INTO track_versions(id,track_id,version_label,content_fingerprint,created_at) VALUES('v','t','original','fingerprint','now'),('other','t','other',NULL,'now');")?;
            conn.execute("INSERT INTO media_sources(id,version_id,source_kind,absolute_path,format,availability,created_at,updated_at) VALUES('s','v','local_imported',?1,'wav','available','now','now')",[file.to_string_lossy().as_ref()])?; Ok(()) }).unwrap();
        let key = db.audio_subtitle_key("v").unwrap();
        let original = vec![SubtitleCueDto {
            index: 7,
            start_ms: 123,
            end_ms: 999,
            text: "こんにちは".into(),
        }];
        let mut doc = SubtitleDocument::new("v".into(), key.clone(), original.clone(), false);
        doc.completed_chunks.insert(0);
        doc.translations.insert(7, "你好".into());
        let audio = vec![0.00001; 16000];
        let assessed = crate::services::asr_quality::recognize(
            crate::services::asr_quality::Region {
                samples: &audio,
                offset_ms: 0,
                begin: 0,
                end: audio.len(),
                model: "fixture",
                language: "ja",
                prompt: "",
                context_samples: 32000,
            },
            &std::sync::atomic::AtomicBool::new(false),
            |_, _| {
                Ok(crate::domain::asr::RecognitionEvidence {
                    text: "hello hello hello hello hello hello".into(),
                    ..Default::default()
                })
            },
        )
        .unwrap();
        doc.asr_records.insert(7, assessed[0].record.clone());
        db.save_subtitle_document(&doc).unwrap();
        drop(db);
        let db = Database::open_and_migrate(&data, &cache).unwrap();
        let saved = db.subtitle_document("v", &key).unwrap().unwrap();
        assert_eq!(saved.cue_status(7), "audioReview");
        assert_eq!(saved.asr_records[&7].attempts.len(), 2);
        assert_eq!(
            saved.asr_records[&7].attempts[0].repetition_ratio,
            Some(1.0)
        );
        assert!(!saved.can_translate(7));
        assert_eq!(
            saved.completed_chunks,
            std::collections::BTreeSet::from([0])
        );
        assert!(!saved.recognition_complete);
        assert_eq!(db.subtitle_overlay("v", vec![])[0].text, "こんにちは\n你好");
        assert!(db.subtitle_document("other", &key).unwrap().is_none());
        std::fs::write(&file, b"changed audio").unwrap();
        assert_ne!(db.audio_subtitle_key("v").unwrap(), key);
        assert!(db.subtitle_overlay("v", vec![]).is_empty());
        doc.source_key = external_key(&original);
        doc.recognition_complete = true;
        db.save_subtitle_document(&doc).unwrap();
        assert_eq!(db.subtitle_overlay("v", original.clone()), doc.rendered());
        let mut shifted = original;
        shifted[0].start_ms += 500;
        assert_eq!(db.subtitle_overlay("v", shifted.clone()), shifted);
    }
}
impl SubtitleDocument {
    pub fn new(
        version_id: String,
        source_key: String,
        original: Vec<SubtitleCueDto>,
        complete: bool,
    ) -> Self {
        Self {
            version_id,
            source_key,
            original,
            translations: BTreeMap::new(),
            asr_records: BTreeMap::new(),
            translation_failures: BTreeMap::new(),
            completed_chunks: BTreeSet::new(),
            recognition_complete: complete,
            translation_key: String::new(),
            original_corrections: BTreeMap::new(),
            model_corrections: BTreeMap::new(),
            model_reviews: BTreeMap::new(),
            reference_subtitles: Vec::new(),
            rule_corrections: BTreeMap::new(),
            rules_suppressed: false,
            translation_corrections: BTreeMap::new(),
            non_dialogue_cues: BTreeSet::new(),
            dialogue_filter_version: 1,
        }
    }
    pub fn rendered(&self) -> Vec<SubtitleCueDto> {
        self.original
            .iter()
            .filter(|cue| !self.dialogue_text(cue).trim().is_empty())
            .filter(|cue| {
                !self.original_text(cue).is_empty() || self.translation_text(cue.index).is_some()
            })
            .filter(|cue| {
                !self
                    .asr_records
                    .get(&cue.index)
                    .is_some_and(|r| r.confirmed_no_speech && !r.human_confirmed)
                    || self.trusted_reference(cue.index).is_some()
            })
            .map(|cue| {
                let translated = self.translation_for_cue(cue).filter(|s| !s.is_empty());
                let mut cue = cue.clone();
                cue.text = self.dialogue_text(&cue);
                if let Some(translated) = translated
                {
                    if translated != cue.text {
                        cue.text.push('\n');
                        cue.text.push_str(translated);
                    }
                }
                cue
            })
            .collect()
    }
    pub fn original_text<'a>(&'a self, cue: &'a SubtitleCueDto) -> &'a str {
        self.original_corrections
            .get(&cue.index)
            .or_else(|| self.rule_corrections.get(&cue.index))
            .or_else(|| self.model_corrections.get(&cue.index))
            .map(String::as_str)
            .unwrap_or(&cue.text)
    }
    pub fn dialogue_text(&self, cue: &SubtitleCueDto) -> String {
        if self.non_dialogue_cues.contains(&cue.index) { return String::new(); }
        let text = self.original_text(cue);
        if !self.source_key.starts_with("sensevoice:")
            || self.original_corrections.contains_key(&cue.index)
            || self.translation_corrections.contains_key(&cue.index)
            || self.trusted_reference(cue.index).is_some() {
            return text.to_owned();
        }
        if self.asr_records.get(&cue.index).is_some_and(|r| !r.human_confirmed && r.dialogue_review.as_ref()
            .is_some_and(|review| review.decision == crate::domain::asr::DialogueDecision::NonDialogue)) {
            return String::new();
        }
        crate::domain::asr::dialogue_only(text)
    }
    pub fn prepare_dialogue_only(&mut self) {
        if self.dialogue_filter_version >= 1 { return; }
        for cue in &self.original {
            if self.dialogue_text(cue) != self.original_text(cue) {
                self.translations.remove(&cue.index);
                self.translation_failures.remove(&cue.index);
            }
        }
        self.dialogue_filter_version = 1;
    }
    pub fn clear_automatic_translations(&mut self) {
        self.translations.clear();
        self.translation_failures.clear();
        self.translation_key.clear();
    }
    pub fn translation_text(&self, index: usize) -> Option<&str> {
        let cue = self.original.iter().find(|cue| cue.index == index)?;
        self.translation_for_cue(cue)
    }
    fn translation_for_cue(&self, cue: &SubtitleCueDto) -> Option<&str> {
        let index = cue.index;
        if self.dialogue_text(cue).trim().is_empty() { return None; }
        self.translation_corrections
            .get(&index)
            .map(String::as_str)
            .or_else(|| self.trusted_reference(index))
            .or_else(|| (self.dialogue_filter_version >= 1 || self.dialogue_text(cue) == self.original_text(cue))
                .then(|| self.translations.get(&index).map(String::as_str)).flatten())
    }
    pub fn trusted_reference(&self, index: usize) -> Option<&str> {
        // A reference is already a Chinese subtitle, independent of an AI
        // translation cache. Keep it visible after cache invalidation/reload.
        let reference = self.reference_subtitles.get(index)?;
        let cue = self.original.iter().find(|cue| cue.index == index)?;
        (cue.start_ms == reference.start_ms && cue.end_ms == reference.end_ms)
            .then_some(reference.text.as_str())
    }
    pub fn review_pending_count(&self) -> usize {
        self.original
            .iter()
            .filter(|cue| !self.dialogue_text(cue).is_empty())
            .filter(|cue| {
                self.model_reviews
                    .get(&cue.index)
                    .is_some_and(|r| !r.note.is_empty())
                    || self
                        .asr_records
                        .get(&cue.index)
                        .is_some_and(|r| r.pending_audio_review && !r.human_confirmed)
            })
            .count()
    }
    pub fn can_translate(&self, id: usize) -> bool {
        self.original
            .iter()
            .find(|c| c.index == id)
            .is_some_and(|c| !self.dialogue_text(c).trim().is_empty())
            && !self.asr_records.get(&id).is_some_and(|r| {
                !r.human_confirmed && (r.pending_audio_review || r.confirmed_no_speech)
            })
    }
    pub fn cue_status(&self, id: usize) -> &'static str {
        if self.original.iter().find(|cue| cue.index == id).is_some_and(|cue|
            self.non_dialogue_cues.contains(&id) || !self.original_text(cue).trim().is_empty() && self.dialogue_text(cue).trim().is_empty()) {
            return "nonDialogue";
        }
        if let Some(r) = self.asr_records.get(&id).filter(|r| !r.human_confirmed) {
            if r.confirmed_no_speech {
                return "noSpeech";
            }
            if r.pending_audio_review {
                return "audioReview";
            }
        }
        if self
            .translation_text(id)
            .is_some_and(|s| !s.trim().is_empty())
        {
            "complete"
        } else if self.translation_failures.contains_key(&id) {
            "translationFailed"
        } else {
            "pendingTranslation"
        }
    }
    pub fn revision(&self) -> String {
        hex::encode(Sha256::digest(
            serde_json::to_vec(self).expect("subtitle document serializes"),
        ))
    }
    pub fn translated_count(&self) -> usize {
        self.original
            .iter()
            .filter(|cue| self.translation_for_cue(cue).is_some())
            .count()
    }
    pub fn dialogue_count(&self) -> usize {
        self.original.iter().filter(|cue| !self.dialogue_text(cue).trim().is_empty()).count()
    }
    pub fn effective_original(&self) -> Vec<SubtitleCueDto> {
        self.original
            .iter()
            .map(|cue| SubtitleCueDto {
                text: self.original_text(cue).into(),
                ..cue.clone()
            })
            .collect()
    }
    pub fn validate(&self) -> AppResult<()> {
        let ids: BTreeSet<_> = self.original.iter().map(|c| c.index).collect();
        if self.version_id.is_empty()
            || !(self.source_key.starts_with("external:")
                || self.source_key.starts_with("sensevoice:"))
            || self.original.len() > 100_000
            || self.reference_subtitles.len() > 100_000
            || self
                .reference_subtitles
                .iter()
                .any(|c| c.start_ms < 0 || c.end_ms <= c.start_ms || c.text.len() > 100_000)
            || ids.len() != self.original.len()
            || self.asr_records.iter().any(|(id, r)| {
                !ids.contains(id)
                    || r.attempts.is_empty()
                    || r.selected_attempt >= r.attempts.len()
                    || r.attempts.len() > 100_000
                    || r.attempts.iter().any(|a| {
                        a.start_ms < 0 || a.end_ms <= a.start_ms || a.result.text.len() > 100_000
                    })
                    || r.dialogue_review.as_ref().is_some_and(|review|
                        review.note.len() > 6000 || review.observations.len() > 5
                        || review.observations.iter().any(|o| o.start_ms < 0 || o.end_ms <= o.start_ms
                            || o.end_ms-o.start_ms > 20_000 || o.result.text.len() > 100_000))
            })
            || self.translation_failures.keys().any(|id| !ids.contains(id))
            || self.non_dialogue_cues.iter().any(|id| !ids.contains(id))
            || self.model_reviews.iter().any(|(id, record)| {
                !ids.contains(id)
                    || record.note.len() > 6000
                    || record.context_count > record.total_count
                    || record.total_count > 100_000
            })
            || self
                .original
                .iter()
                .any(|c| c.start_ms < 0 || c.end_ms <= c.start_ms || c.text.len() > 100_000)
            || [
                &self.translations,
                &self.original_corrections,
                &self.model_corrections,
                &self.rule_corrections,
                &self.translation_corrections,
            ]
            .iter()
            .any(|map| {
                map.iter()
                    .any(|(id, text)| !ids.contains(id) || text.len() > 100_000)
            })
        {
            return Err(AppError::new(
                ErrorCode::InvalidInput,
                "字幕文档包含无效的句 ID、时间轴或文本",
            ));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubtitleBackupRecord {
    pub document: SubtitleDocument,
    pub updated_at: String,
}
pub fn external_key(cues: &[SubtitleCueDto]) -> String {
    format!(
        "external:{}",
        hex::encode(Sha256::digest(
            serde_json::to_vec(cues).expect("subtitle cues serialize")
        ))
    )
}
impl Database {
    /// T1519: a work badge means every audio version has a current, complete ASR
    /// document. Empty/silent versions count as completed, but a wholly empty work
    /// does not advertise subtitles. External documents never count as recognition.
    pub fn recognized_subtitle_works(&self, work: Option<&str>) -> AppResult<BTreeSet<String>> {
        let (totals, documents) = self.with_conn(|conn| {
            // T1515: Only works with completed recognition documents can have this
            // badge. Avoid counting every version when there are no candidates.
            let mut stmt = conn.prepare("WITH candidates AS (
                SELECT DISTINCT t.work_id FROM subtitle_documents d
                CROSS JOIN track_versions v ON v.id=d.version_id CROSS JOIN tracks t ON t.id=v.track_id
                WHERE (?1 IS NULL OR t.work_id=?1) AND d.source_key LIKE 'sensevoice:%'
                  AND json_extract(d.document_json,'$.recognitionComplete')=1
                  AND COALESCE(json_array_length(d.document_json,'$.referenceSubtitles'),0)=0
            ) SELECT t.work_id,COUNT(*) FROM candidates c CROSS JOIN tracks t ON t.work_id=c.work_id
              CROSS JOIN track_versions v ON v.track_id=t.id
              WHERE EXISTS(SELECT 1 FROM media_sources s WHERE s.version_id=v.id) GROUP BY t.work_id")?;
            let totals = stmt.query_map([work], |r| Ok((r.get::<_,String>(0)?, r.get::<_,usize>(1)?)))?.collect::<Result<BTreeMap<_,_>,_>>()?;
            // Keep the sparse document table as the outer loop even before ANALYZE.
            let mut stmt = conn.prepare("SELECT t.work_id,d.version_id,d.source_key,json_array_length(d.document_json,'$.original') FROM subtitle_documents d CROSS JOIN track_versions v ON v.id=d.version_id CROSS JOIN tracks t ON t.id=v.track_id WHERE (?1 IS NULL OR t.work_id=?1) AND d.source_key LIKE 'sensevoice:%' AND json_extract(d.document_json,'$.recognitionComplete')=1 AND COALESCE(json_array_length(d.document_json,'$.referenceSubtitles'),0)=0")?;
            let documents = stmt.query_map([work], |r| Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,usize>(3)?)))?.collect::<Result<Vec<_>,_>>()?;
            Ok((totals, documents))
        })?;
        let mut completed: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        let mut nonempty = BTreeSet::new();
        let mut keys = BTreeMap::new();
        for (work, version, key, cues) in documents {
            let current = keys
                .entry(version.clone())
                .or_insert_with(|| self.audio_subtitle_key(&version).ok());
            if current.as_deref() == Some(&key) {
                completed.entry(work.clone()).or_default().insert(version);
                if cues > 0 {
                    nonempty.insert(work);
                }
            }
        }
        Ok(completed
            .into_iter()
            .filter_map(|(work, versions)| {
                (totals.get(&work) == Some(&versions.len()) && nonempty.contains(&work))
                    .then_some(work)
            })
            .collect())
    }

    pub fn audio_subtitle_key(&self, version: &str) -> AppResult<String> {
        let identity = self.with_conn(|conn| {
            let version_hash: Option<String> = conn.query_row("SELECT content_fingerprint FROM track_versions WHERE id=?1", [version], |r| r.get(0))?;
            let mut stmt = conn.prepare("SELECT id,absolute_path,content_sha256,remote_file_id,byte_size FROM media_sources WHERE version_id=?1 AND availability='available' ORDER BY CASE source_kind WHEN 'local_imported' THEN 0 WHEN 'local_downloaded' THEN 1 ELSE 2 END,id")?;
            let rows = stmt.query_map([version], |r| Ok((r.get::<_,String>(0)?,r.get::<_,Option<String>>(1)?,r.get::<_,Option<String>>(2)?,r.get::<_,Option<String>>(3)?,r.get::<_,Option<i64>>(4)?)))?;
            for row in rows {
                let (id,path,hash,remote,size) = row?;
                if let Some(path) = path {
                    if path.ends_with(".part") { continue; }
                    if let Ok(meta) = std::fs::metadata(path) {
                        let modified = meta.modified().ok().and_then(|v|v.duration_since(std::time::UNIX_EPOCH).ok()).map(|v|v.as_nanos()).unwrap_or(0);
                        return Ok(format!("{id}:{}:{modified}:{}:{version_hash:?}",meta.len(),hash.unwrap_or_default()));
                    }
                } else if let Some(remote) = remote { return Ok(format!("{id}:{remote}:{size:?}:{}:{version_hash:?}", hash.unwrap_or_default())); }
            }
            Err(AppError::new(ErrorCode::SourceNotFound,"没有可读取的识别音源"))
        })?;
        Ok(format!(
            "sensevoice:int8-v1:{}",
            hex::encode(Sha256::digest(identity))
        ))
    }
    pub fn subtitle_document(
        &self,
        version: &str,
        key: &str,
    ) -> AppResult<Option<SubtitleDocument>> {
        let raw: Option<String> = self.with_conn(|conn| Ok(conn.query_row("SELECT document_json FROM subtitle_documents WHERE version_id=?1 AND source_key=?2",params![version,key],|r|r.get(0)).optional()?))?;
        raw.map(|raw| {
            serde_json::from_str(&raw)
                .map_err(|_| AppError::new(ErrorCode::DatabaseError, "保存的识别字幕无法读取"))
        })
        .transpose()
    }
    pub fn save_subtitle_document(&self, document: &SubtitleDocument) -> AppResult<()> {
        self.with_conn(|conn| {
            let mut document = document.clone();
            let saved: Option<String> = conn.query_row("SELECT document_json FROM subtitle_documents WHERE version_id=?1 AND source_key=?2",params![document.version_id,document.source_key],|r|r.get(0)).optional()?;
            if let Some(raw) = saved {
                let saved: SubtitleDocument = serde_json::from_str(&raw).map_err(|_| AppError::new(ErrorCode::DatabaseError,"保存的字幕无法读取"))?;
                // Background resumes and restores must never replace manual corrections.
                for (id, text) in &saved.original_corrections {
                    if document.original_corrections.get(id) != Some(text) { document.translations.remove(id); }
                }
                document.original_corrections = saved.original_corrections;
                document.translation_corrections = saved.translation_corrections;
                for (id, record) in &saved.asr_records {
                    if record.human_confirmed && document.original.iter().any(|cue| cue.index == *id && saved.original.contains(cue)) {
                        if let Some(current) = document.asr_records.get_mut(id) { current.human_confirmed = true; }
                    }
                }
                for id in saved.non_dialogue_cues {
                    if document.original.iter().any(|cue| cue.index == id && saved.original.contains(cue)) {
                        document.non_dialogue_cues.insert(id);
                    }
                }
            }
            document.prepare_dialogue_only();
            Self::write_subtitle_document(conn, &document)
        })
    }
    pub fn clear_automatic_translations_for_version(&self, version: &str) -> AppResult<()> {
        self.with_conn(|conn| {
            let tx = conn.transaction()?;
            let rows: Vec<String> = {
                let mut query = tx.prepare("SELECT document_json FROM subtitle_documents WHERE version_id=?1")?;
                let rows = query.query_map([version], |row| row.get(0))?.collect::<Result<_, _>>()?;
                rows
            };
            for raw in rows {
                let mut doc: SubtitleDocument = serde_json::from_str(&raw)
                    .map_err(|_| AppError::new(ErrorCode::DatabaseError, "保存的字幕无法读取"))?;
                if !doc.translations.is_empty() || !doc.translation_failures.is_empty() || !doc.translation_key.is_empty() {
                    doc.clear_automatic_translations();
                    Self::write_subtitle_document(&tx, &doc)?;
                }
            }
            tx.commit()?;
            Ok(())
        })
    }
    pub(crate) fn write_subtitle_document(
        conn: &rusqlite::Connection,
        document: &SubtitleDocument,
    ) -> AppResult<()> {
        let raw = serde_json::to_string(document)
            .map_err(|_| AppError::new(ErrorCode::DatabaseError, "无法保存字幕"))?;
        conn.execute("INSERT INTO subtitle_documents(version_id,source_key,document_json,updated_at) VALUES(?1,?2,?3,?4) ON CONFLICT(version_id,source_key) DO UPDATE SET document_json=excluded.document_json,updated_at=excluded.updated_at",params![document.version_id,document.source_key,raw,chrono::Utc::now().to_rfc3339()])?;
        Ok(())
    }
    /// Restore only matching audio or matching external subtitle content. No network or model loading.
    pub fn subtitle_overlay(
        &self,
        version: &str,
        cues: Vec<SubtitleCueDto>,
    ) -> Vec<SubtitleCueDto> {
        if !cues.is_empty() {
            if let Some(doc) = self
                .audio_subtitle_key(version)
                .ok()
                .and_then(|key| self.subtitle_document(version, &key).ok().flatten())
            {
                // Reloading the same Chinese subtitle must restore its bilingual ASR
                // result. Never apply it to another subtitle or a changed audio source.
                if !doc.original.is_empty()
                    && doc.reference_subtitles.len() == cues.len()
                    && doc.reference_subtitles.iter().zip(&cues).all(|(a, b)| {
                        a.start_ms == b.start_ms && a.end_ms == b.end_ms && a.text == b.text
                    })
                {
                    return doc.rendered();
                }
            }
        }
        let key = if cues.is_empty() {
            self.audio_subtitle_key(version).ok()
        } else {
            Some(external_key(&cues))
        };
        key.and_then(|key| self.subtitle_document(version, &key).ok().flatten())
            .map(|doc| doc.rendered())
            .unwrap_or(cues)
    }
    pub fn document_for_display(
        &self,
        version: &str,
        cues: &[SubtitleCueDto],
    ) -> AppResult<Option<SubtitleDocument>> {
        let raw: Vec<String> = self.with_conn(|conn| {
            let mut stmt = conn.prepare("SELECT document_json FROM subtitle_documents WHERE version_id=?1 ORDER BY updated_at DESC")?;
            let result = stmt.query_map([version], |r|r.get::<_,String>(0))?.collect::<Result<Vec<_>,_>>()?; Ok(result)
        })?;
        for row in raw {
            if let Ok(doc) = serde_json::from_str::<SubtitleDocument>(&row) {
                if !cues.is_empty()
                    && (doc.rendered() == cues
                        || !doc.reference_subtitles.is_empty() && doc.effective_original() == cues)
                {
                    return Ok(Some(doc));
                }
            }
        }
        Ok(None)
    }
}
