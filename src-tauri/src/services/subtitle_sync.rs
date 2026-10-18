//! T1519: explicit cross-format subtitle copies, without merging versions or playback facts.
use crate::{
    db::{subtitles::SubtitleDocument, Database},
    domain::{AppError, AppResult, ErrorCode},
    services::{library::LibraryService, subtitle_tasks::SubtitleTaskService},
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

fn invalid(message: &str) -> AppError { AppError::new(ErrorCode::InvalidInput, message) }

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SubtitleSyncCandidate {
    pub source_version_id: String,
    pub target_version_id: String,
    pub source_label: String,
    pub target_label: String,
    pub source_duration_ms: i64,
    pub target_duration_ms: i64,
    pub source_key: String,
    pub target_key: String,
    pub revision: String,
}

struct AudioVersion {
    id: String,
    work: String,
    title: String,
    directory: String,
    duration: i64,
    format: String,
}
fn is_format(value: &str) -> bool {
    matches!(value.to_ascii_lowercase().as_str(), "wav" | "mp3" | "flac" | "m4a" | "aac" | "ogg" | "opus" | "wma" | "aiff" | "alac")
}
fn matching_path(audio: &AudioVersion) -> String {
    let title = audio.title.rsplit_once('.').filter(|(_, ext)| is_format(ext)).map(|(stem, _)| stem).unwrap_or(&audio.title);
    let directory = audio.directory.replace('\\', "/").split('/')
        .filter(|part| !is_format(part)).collect::<Vec<_>>().join("/");
    format!("{directory}/{title}").to_lowercase()
}
fn matches_audio(source: &AudioVersion, target: &AudioVersion) -> bool {
    source.work == target.work && source.id != target.id
        && is_format(&source.format) && is_format(&target.format)
        && !source.format.eq_ignore_ascii_case(&target.format)
        && source.duration > 0 && target.duration > 0
        && (source.duration - target.duration).abs() <= 250.min(source.duration / 1000)
        && matching_path(source) == matching_path(target)
}
impl Database {
    pub fn preview_subtitle_sync(&self, sources: &[String]) -> AppResult<Vec<SubtitleSyncCandidate>> {
        if sources.is_empty() || sources.len() > 1000 { return Err(invalid("请选择 1–1000 条源音轨")); }
        let selected: BTreeSet<_> = sources.iter().cloned().collect();
        let versions = self.with_conn(|conn| {
            let mut stmt = conn.prepare("SELECT v.id,t.work_id,t.title,t.logical_dir_path,COALESCE(v.duration_ms,0),COALESCE((SELECT format FROM media_sources s WHERE s.version_id=v.id AND s.availability='available' ORDER BY CASE s.source_kind WHEN 'local_imported' THEN 0 WHEN 'local_downloaded' THEN 1 ELSE 2 END,s.id LIMIT 1),'') FROM track_versions v JOIN tracks t ON t.id=v.track_id WHERE t.work_id IN (SELECT t2.work_id FROM tracks t2 JOIN track_versions v2 ON v2.track_id=t2.id WHERE v2.id IN (SELECT value FROM json_each(?1)))")?;
            let rows = stmt.query_map([serde_json::to_string(sources).map_err(|_| invalid("音轨选择无效"))?], |r| Ok(AudioVersion { id: r.get(0)?, work: r.get(1)?, title: r.get(2)?, directory: r.get(3)?, duration: r.get(4)?, format: r.get(5)? }))?.collect::<Result<Vec<_>,_>>()?;
            Ok(rows)
        })?;
        let library = LibraryService::new(self.clone());
        let mut candidates = Vec::new();
        for source in versions.iter().filter(|v| selected.contains(&v.id)) {
            let Ok(key) = self.audio_subtitle_key(&source.id) else { continue; };
            let Some(doc) = self.subtitle_document(&source.id, &key)?.filter(|d| d.recognition_complete) else { continue; };
            for target in versions.iter().filter(|v| !selected.contains(&v.id) && matches_audio(source, v)) {
                if library.find_best_subtitle_for_version(&target.id)?.is_some() { continue; }
                let Ok(target_key) = self.audio_subtitle_key(&target.id) else { continue; };
                if self.subtitle_document(&target.id, &target_key)?.is_some() { continue; }
                candidates.push(SubtitleSyncCandidate {
                    source_version_id: source.id.clone(), target_version_id: target.id.clone(),
                    source_label: format!("{}/{} · {}", source.directory, source.title, source.format.to_uppercase()),
                    target_label: format!("{}/{} · {}", target.directory, target.title, target.format.to_uppercase()),
                    source_duration_ms: source.duration, target_duration_ms: target.duration,
                    source_key: key.clone(), target_key, revision: doc.revision(),
                });
            }
        }
        // Ambiguous same-name source versions must never be chosen arbitrarily.
        let mut counts = std::collections::BTreeMap::new();
        for candidate in &candidates { *counts.entry(candidate.target_version_id.clone()).or_insert(0) += 1; }
        candidates.retain(|c| counts[&c.target_version_id] == 1);
        candidates.sort_by(|a,b| a.target_label.cmp(&b.target_label));
        Ok(candidates)
    }

    pub fn apply_subtitle_sync(&self, candidates: &[SubtitleSyncCandidate]) -> AppResult<usize> {
        if candidates.is_empty() || candidates.len() > 1000 { return Err(invalid("请选择要同步的音轨")); }
        let targets: BTreeSet<_> = candidates.iter().map(|c| &c.target_version_id).collect();
        if targets.len() != candidates.len() { return Err(invalid("不能重复选择目标音轨")); }
        let sources: Vec<_> = candidates.iter().map(|c| c.source_version_id.clone()).collect::<BTreeSet<_>>().into_iter().collect();
        let fresh = self.preview_subtitle_sync(&sources)?;
        if candidates.iter().any(|c| !fresh.contains(c)) { return Err(invalid("音源或字幕已变化，请重新预览同步")); }
        self.with_conn(|conn| {
            let tx = conn.transaction()?;
            for candidate in candidates {
                let raw: String = tx.query_row("SELECT document_json FROM subtitle_documents WHERE version_id=?1 AND source_key=?2", rusqlite::params![candidate.source_version_id, candidate.source_key], |r| r.get(0))?;
                let mut doc: SubtitleDocument = serde_json::from_str(&raw).map_err(|_| invalid("源字幕无法读取"))?;
                if doc.revision() != candidate.revision { return Err(invalid("源字幕已变化，请重新预览同步")); }
                let exists: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM subtitle_documents WHERE version_id=?1 AND source_key=?2)", rusqlite::params![candidate.target_version_id, candidate.target_key], |r| r.get(0))?;
                if exists { return Err(invalid("目标已有字幕，请重新预览同步")); }
                doc.version_id = candidate.target_version_id.clone();
                doc.source_key = candidate.target_key.clone();
                doc.completed_chunks.clear();
                doc.validate()?;
                Self::write_subtitle_document(&tx, &doc)?;
            }
            tx.commit()?;
            Ok(candidates.len())
        })
    }
}
impl SubtitleTaskService {
    pub fn preview_sync(&self, versions: &[String]) -> AppResult<Vec<SubtitleSyncCandidate>> {
        self.with_idle(|| self.db.preview_subtitle_sync(versions))
    }
    pub fn sync_subtitles(&self, candidates: &[SubtitleSyncCandidate]) -> AppResult<usize> {
        self.with_idle(|| {
            let count = self.db.apply_subtitle_sync(candidates)?;
            self.refresh_restored_subtitles();
            let works: BTreeSet<_> = candidates.iter().filter_map(|c| self.subtitle_work_context(Some(&c.target_version_id)).ok().map(|context| context.work_id)).collect();
            for work in works { self.playback.notify_affinity(&work); }
            Ok(count)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::SubtitleCueDto;
    fn fixture() -> (tempfile::TempDir, Database, SubtitleDocument) {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open_and_migrate(&dir.path().join("data"), &dir.path().join("cache")).unwrap();
        db.with_conn(|conn| { conn.execute_batch("INSERT INTO works(id,title,created_at,updated_at) VALUES('w','work','now','now'); INSERT INTO collected_works(work_id,created_at) VALUES('w','now');")?; Ok(()) }).unwrap();
        add_audio(&db, "wav", "WAV", "01.wav", "wav", 60_000);
        add_audio(&db, "mp3", "MP3", "01.mp3", "mp3", 60_020);
        let key = db.audio_subtitle_key("wav").unwrap();
        let mut doc = SubtitleDocument::new("wav".into(), key, vec![SubtitleCueDto { index: 4, start_ms: 1000, end_ms: 2000, text: "こんにちは".into() }], true);
        doc.translations.insert(4, "你好".into());
        doc.original_corrections.insert(4, "こんばんは".into());
        doc.translation_corrections.insert(4, "晚上好".into());
        db.save_subtitle_document(&doc).unwrap();
        (dir, db, doc)
    }
    fn add_audio(db: &Database, id: &str, directory: &str, title: &str, format: &str, duration: i64) {
        db.with_conn(|conn| {
            conn.execute("INSERT INTO tracks(id,work_id,title,logical_dir_path,sort_key,created_at) VALUES(?1,'w',?2,?3,?1,'now')", rusqlite::params![id,title,directory])?;
            conn.execute("INSERT INTO track_versions(id,track_id,version_label,duration_ms,created_at) VALUES(?1,?1,?2,?3,'now')", rusqlite::params![id,format,duration])?;
            conn.execute("INSERT INTO media_sources(id,version_id,source_kind,remote_file_id,format,availability,created_at,updated_at) VALUES(?1,?1,'online_provider',?1,?2,'available','now','now')", rusqlite::params![id,format])?;
            Ok(())
        }).unwrap();
    }
    #[test]
    fn t1519_copy_preserves_text_and_timeline_without_merging_and_sets_complete_badge() {
        let (dir, db, doc) = fixture();
        assert!(!db.get_work_detail("w").unwrap().has_recognized_subtitle);
        assert!(!db.list_works("all", None).unwrap()[0].has_subtitle);
        let plan = db.preview_subtitle_sync(&["wav".into()]).unwrap();
        assert_eq!(plan.len(), 1);
        assert!(db.subtitle_overlay("mp3", vec![]).is_empty()); // preview has no writes
        assert_eq!(db.apply_subtitle_sync(&plan).unwrap(), 1);
        assert_eq!(db.subtitle_overlay("mp3", vec![]), doc.rendered());
        assert!(db.get_work_detail("w").unwrap().has_recognized_subtitle);
        let summary = &db.list_works("all", None).unwrap()[0];
        assert!(summary.has_recognized_subtitle && summary.has_subtitle);
        assert_eq!(db.get_work_detail("w").unwrap().tracks.len(), 2);
        assert!(db.preview_subtitle_sync(&["wav".into()]).unwrap().is_empty());
        assert!(db.apply_subtitle_sync(&plan).is_err());
        drop(db);
        let db = Database::open_and_migrate(&dir.path().join("data"), &dir.path().join("cache")).unwrap();
        assert_eq!(db.subtitle_overlay("mp3", vec![]), doc.rendered());
        assert!(db.recognized_subtitle_works(None).unwrap().contains("w"));
        db.with_conn(|conn| { conn.execute("UPDATE media_sources SET remote_file_id='changed-audio' WHERE id='mp3'", [])?; Ok(()) }).unwrap();
        assert!(db.subtitle_overlay("mp3", vec![]).is_empty());
        assert!(!db.get_work_detail("w").unwrap().has_recognized_subtitle);
    }
    #[test]
    fn t1519_partial_external_or_changed_documents_are_not_full_recognition() {
        let (_dir, db, mut doc) = fixture();
        doc.recognition_complete = false;
        db.save_subtitle_document(&doc).unwrap();
        assert!(db.preview_subtitle_sync(&["wav".into()]).unwrap().is_empty());
        assert!(db.recognized_subtitle_works(None).unwrap().is_empty());
        doc.recognition_complete = true;
        db.save_subtitle_document(&doc).unwrap();
        let plan = db.preview_subtitle_sync(&["wav".into()]).unwrap();
        doc.translations.insert(4, "修改译文".into());
        db.save_subtitle_document(&doc).unwrap();
        assert!(db.apply_subtitle_sync(&plan).is_err());
        assert!(db.subtitle_overlay("mp3", vec![]).is_empty());
        let mut external = doc.clone();
        external.version_id = "mp3".into();
        external.source_key = crate::db::subtitles::external_key(&external.original);
        db.save_subtitle_document(&external).unwrap();
        assert!(db.recognized_subtitle_works(None).unwrap().is_empty());
        // Partial target ASR is protected, even though it cannot set the work badge.
        external.source_key = db.audio_subtitle_key("mp3").unwrap();
        external.recognition_complete = false;
        db.save_subtitle_document(&external).unwrap();
        assert!(db.preview_subtitle_sync(&["wav".into()]).unwrap().is_empty());
    }
    #[test]
    fn t1515_recognition_candidates_keep_work_scope_and_partial_work_counts() {
        let (_dir, db, doc) = fixture();
        db.with_conn(|conn| {
            conn.execute_batch("INSERT INTO works(id,title,created_at,updated_at) VALUES('other','Other work','now','now');")?;
            Ok(())
        }).unwrap();
        add_audio(&db, "other-audio", "WAV", "other.wav", "wav", 60000);
        db.with_conn(|conn| {
            conn.execute_batch("UPDATE tracks SET work_id='other' WHERE id='other-audio';
                INSERT INTO media_sources(id,version_id,source_kind,remote_file_id,format,availability,created_at,updated_at)
                VALUES('zz-second-source','wav','online_provider','wav','wav','available','now','now');")?;
            Ok(())
        }).unwrap();
        let mut other = doc.clone();
        other.version_id = "other-audio".into();
        other.source_key = db.audio_subtitle_key(&other.version_id).unwrap();
        db.save_subtitle_document(&other).unwrap();
        assert_eq!(db.recognized_subtitle_works(None).unwrap(), BTreeSet::from(["other".into()]));
        assert!(db.recognized_subtitle_works(Some("w")).unwrap().is_empty());
        assert_eq!(db.recognized_subtitle_works(Some("other")).unwrap(), BTreeSet::from(["other".into()]));
        let mut remaining = doc;
        remaining.version_id = "mp3".into();
        remaining.source_key = db.audio_subtitle_key("mp3").unwrap();
        db.save_subtitle_document(&remaining).unwrap();
        assert_eq!(db.recognized_subtitle_works(None).unwrap(), BTreeSet::from(["w".into(), "other".into()]));
        assert!(db.recognized_subtitle_works(Some("missing")).unwrap().is_empty());
    }
    #[test]
    fn t1513_reference_only_recognition_is_not_a_whole_work_badge() {
        let (_dir,db,mut doc)=fixture();
        let plan=db.preview_subtitle_sync(&["wav".into()]).unwrap();
        db.apply_subtitle_sync(&plan).unwrap();
        assert!(db.recognized_subtitle_works(None).unwrap().contains("w"));
        doc.reference_subtitles=doc.original.clone();
        db.save_subtitle_document(&doc).unwrap();
        assert!(db.recognized_subtitle_works(None).unwrap().is_empty());
    }
    #[test]
    fn t1519_matching_preserves_edition_paths_rejects_duration_and_ambiguity() {
        let (_dir, db, doc) = fixture();
        add_audio(&db, "no-se", "SEなし/FLAC", "01.flac", "flac", 60_000);
        add_audio(&db, "short", "OGG", "01.ogg", "ogg", 30_000);
        add_audio(&db, "unknown", "AAC", "01.aac", "aac", 0);
        add_audio(&db, "different-name", "OPUS", "02.opus", "opus", 60_000);
        assert_eq!(db.preview_subtitle_sync(&["wav".into()]).unwrap().len(), 1);
        add_audio(&db, "other-source", "FLAC", "01.flac", "flac", 60_000);
        let mut other = doc;
        other.version_id = "other-source".into();
        other.source_key = db.audio_subtitle_key(&other.version_id).unwrap();
        db.save_subtitle_document(&other).unwrap();
        assert!(db.preview_subtitle_sync(&["wav".into(), "other-source".into()]).unwrap().is_empty());
    }
    #[test]
    fn t1519_existing_external_subtitle_and_wholly_empty_work_do_not_get_replaced_or_badged() {
        let (_dir, db, _) = fixture();
        db.with_conn(|conn| { conn.execute_batch("INSERT INTO attachments(id,work_id,attachment_type,title,relative_path,availability,created_at) VALUES('sub','w','subtitle','01.srt','MP3/01.srt','available','now');")?; Ok(()) }).unwrap();
        assert!(db.preview_subtitle_sync(&["wav".into()]).unwrap().is_empty());
        db.with_conn(|conn| { conn.execute("DELETE FROM subtitle_documents", [])?; Ok(()) }).unwrap();
        for version in ["wav", "mp3"] {
            db.save_subtitle_document(&SubtitleDocument::new(version.into(), db.audio_subtitle_key(version).unwrap(), vec![], true)).unwrap();
        }
        assert!(db.recognized_subtitle_works(None).unwrap().is_empty());
        assert!(db.get_work_detail("w").unwrap().has_subtitle); // the original external badge remains
    }
}
