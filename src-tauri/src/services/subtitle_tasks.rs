use crate::{
    db::{
        subtitles::{external_key, SubtitleDocument},
        Database,
    },
    domain::{AppError, AppResult, ErrorCode, SubtitleCueDto},
    providers::{AsmrOneProvider, MediaPurpose, ProviderMediaRef},
    services::{
        playback::PlaybackService,
        recognition::{
            AutoRecognitionMode, RecognitionService, RecognitionSettings, TranslationLine,
            LOCAL_BATCH_CHARS, LOCAL_BATCH_SIZE,
        },
        speech_engine::{analysis_context_ms, decode_chunk, probe_duration, SenseVoice},
        speech_segments::{prefetch_chunks, CHUNK_MS},
    },
};
use parking_lot::Mutex;
use serde::Serialize;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

type AutomaticAttempt = (u64, String, RecognitionSettings, usize, usize);
#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubtitleTaskSnapshot {
    pub automatic: bool,
    pub version_id: Option<String>,
    pub state: String,
    pub phase: String,
    pub completed: usize,
    pub total: usize,
    pub error: Option<String>,
    pub tracks_completed: usize,
    pub tracks_total: usize,
    pub review_pending: usize,
}
#[derive(Default)]
struct Task {
    snapshot: SubtitleTaskSnapshot,
    cancel: Option<Arc<AtomicBool>>,
}
#[derive(Clone)]
pub struct SubtitleTaskService {
    pub(super) db: Database,
    pub(super) recognition: RecognitionService,
    pub(super) playback: PlaybackService,
    pub(super) provider: AsmrOneProvider,
    task: Arc<Mutex<Task>>,
    stopping: Arc<AtomicBool>,
    suppressed_generation: Arc<Mutex<Option<u64>>>,
}
fn error(message: &str) -> AppError {
    AppError::new(ErrorCode::InvalidInput, message)
}
impl SubtitleTaskService {
    pub fn new(
        db: Database,
        recognition: RecognitionService,
        playback: PlaybackService,
        provider: AsmrOneProvider,
    ) -> Self {
        Self {
            db,
            recognition,
            playback,
            provider,
            task: Default::default(),
            stopping: Default::default(),
            suppressed_generation: Default::default(),
        }
    }
    pub fn snapshot(&self) -> SubtitleTaskSnapshot {
        self.task.lock().snapshot.clone()
    }
    pub fn cancel(&self) {
        *self.suppressed_generation.lock() = Some(self.playback.subtitle_context().0);
        let mut task = self.task.lock();
        if let Some(cancel) = &task.cancel {
            cancel.store(true, Ordering::SeqCst);
            task.snapshot.state = "cancelling".into();
        }
    }
    pub fn shutdown(&self) {
        self.stopping.store(true, Ordering::SeqCst);
        self.cancel();
    }
    pub(crate) fn with_idle<T>(&self, action: impl FnOnce() -> AppResult<T>) -> AppResult<T> {
        let task = self.task.lock();
        if task.cancel.is_some() {
            return Err(error("字幕任务正在运行，请先取消或等待完成后操作字幕文档"));
        }
        action()
    }
    pub(super) fn document(&self, version: &str) -> AppResult<SubtitleDocument> {
        let (_, current, current_cues, _) = self.playback.subtitle_context();
        let cues = if current.as_deref() == Some(version) && !current_cues.is_empty() {
            current_cues
        } else {
            let library = crate::services::library::LibraryService::new(self.db.clone());
            library
                .find_best_subtitle_for_version(version)?
                .and_then(|att| library.load_subtitle_cues(&att.id).ok())
                .unwrap_or_default()
        };
        if let Some(doc) = self.db.document_for_display(version, &cues)? {
            return Ok(doc);
        }
        let key = if cues.is_empty() {
            self.db.audio_subtitle_key(version)?
        } else {
            external_key(&cues)
        };
        Ok(self
            .db
            .subtitle_document(version, &key)?
            .unwrap_or_else(|| {
                SubtitleDocument::new(version.into(), key, cues.clone(), !cues.is_empty())
            }))
    }
    fn task_document(
        &self,
        version: &str,
        settings: &RecognitionSettings,
        automatic: bool,
    ) -> AppResult<SubtitleDocument> {
        if automatic && settings.auto_recognition == AutoRecognitionMode::All {
            // Keep external subtitles and their edits in their own document. Full automatic
            // recognition uses the existing audio cache instead of treating an external text as ASR.
            let key = self.db.audio_subtitle_key(version)?;
            return Ok(self
                .db
                .subtitle_document(version, &key)?
                .unwrap_or_else(|| SubtitleDocument::new(version.into(), key, Vec::new(), false)));
        }
        self.document(version)
    }
    fn recognition_document(&self, version: &str, restart: bool) -> AppResult<SubtitleDocument> {
        let key = self.db.audio_subtitle_key(version)?;
        if !restart {
            if let Some(doc) = self.db.subtitle_document(version, &key)? {
                return Ok(doc);
            }
        }
        Ok(SubtitleDocument::new(
            version.into(),
            key,
            Vec::new(),
            false,
        ))
    }
    fn manual_recognition_document(
        &self,
        version: &str,
        _displayed: &SubtitleDocument,
        settings: &RecognitionSettings,
        restart: bool,
    ) -> AppResult<SubtitleDocument> {
        let mut doc = self.recognition_document(version, restart)?;
        let references = if settings.chinese_reference {
            chinese_reference_cues(&self.external_subtitle_cues(version)?)?
        } else { Vec::new() };
        if !doc.reference_subtitles.is_empty() && doc.reference_subtitles != references {
            return Err(error("参考字幕来源已变化，请选择重新识别；程序生成的译文不作为中文字幕参考"));
        }
        if settings.chinese_reference && doc.reference_subtitles.is_empty() {
            // Resolve a real reference before deciding whether existing ASR needs restarting.
            // No Chinese reference is a normal ASR task, even with this preference enabled.
            // Only the attachment itself supplies reference timing/text. Rendered
            // bilingual overlays may contain an incomplete machine translation.
            if !references.is_empty() {
                if !doc.original.is_empty() || !doc.completed_chunks.is_empty() {
                    return Err(error("切换为中文字幕辅助识别时，请选择重新识别"));
                }
                doc.reference_subtitles = references;
            }
        } else if !settings.chinese_reference && !doc.reference_subtitles.is_empty() {
            return Err(error("退出中文字幕辅助流程时，请选择重新识别"));
        }
        Ok(doc)
    }
    fn external_subtitle_cues(&self, version: &str) -> AppResult<Vec<SubtitleCueDto>> {
        let library = crate::services::library::LibraryService::new(self.db.clone());
        library.find_best_subtitle_for_version(version)?
            .map(|att| library.load_subtitle_cues(&att.id))
            .transpose().map(|cues| cues.unwrap_or_default())
    }
    fn automatic_recognition_allowed(
        &self,
        doc: &SubtitleDocument,
        settings: &RecognitionSettings,
    ) -> AppResult<bool> {
        if !doc.reference_subtitles.is_empty() {
            return Ok(false);
        }
        Ok(match settings.auto_recognition {
            AutoRecognitionMode::Off => false,
            AutoRecognitionMode::All => true,
            AutoRecognitionMode::Missing => {
                !doc.source_key.starts_with("external:")
                    && crate::services::library::LibraryService::new(self.db.clone())
                        .find_best_subtitle_for_version(&doc.version_id)?
                        .is_none()
            }
        })
    }
    pub async fn prepare(&self, versions: &[String]) -> AppResult<Vec<String>> {
        validate_versions(versions)?;
        let library = crate::services::library::LibraryService::new(self.db.clone());
        let mut chinese = Vec::new();
        for version in versions {
            if let Some(att) = library.find_best_subtitle_for_version(version)? {
                if library.load_subtitle_cues(&att.id).is_err() {
                    self.provider.ensure_online_subtitle_cached(&att.id).await?;
                }
            }
            if has_chinese(&self.external_subtitle_cues(version)?) {
                chinese.push(version.clone());
            }
        }
        Ok(chinese)
    }
    pub async fn inspect(&self, versions: &[String]) -> AppResult<serde_json::Value> {
        let chinese = self.prepare(versions).await?;
        let mut existing = Vec::new();
        let mut deletable_recognition = Vec::new();
        let mut deletable_translation = Vec::new();
        for version in versions {
            let doc = self.document(version)?;
            if !doc.original.is_empty() || !doc.completed_chunks.is_empty() {
                existing.push(version.clone());
            }
            if self.can_delete_generated(&doc, false)? { deletable_recognition.push(version.clone()); }
            if self.can_delete_generated(&doc, true)? { deletable_translation.push(version.clone()); }
        }
        Ok(serde_json::json!({"existing":existing,"chinese":chinese,
            "deletableRecognition":deletable_recognition,"deletableTranslation":deletable_translation}))
    }
    fn can_delete_generated(&self, doc: &SubtitleDocument, translate: bool) -> AppResult<bool> {
        if translate {
            return Ok(doc.translations.keys().chain(doc.translation_corrections.keys())
                .chain(doc.translation_failures.keys()).any(|id| doc.trusted_reference(*id).is_none()));
        }
        Ok(doc.source_key.starts_with("sensevoice:")
            && (!doc.original.is_empty() || !doc.completed_chunks.is_empty())
            && crate::services::library::LibraryService::new(self.db.clone())
                .find_best_subtitle_for_version(&doc.version_id)?.is_none())
    }
    pub fn delete_generated(&self, versions: &[String], translate: bool) -> AppResult<()> {
        validate_versions(versions)?;
        self.with_idle(|| {
            let (generation, current, expected, _) = self.playback.subtitle_context();
            let mut documents = Vec::new();
            for version in versions {
                if documents.iter().any(|doc: &SubtitleDocument| doc.version_id == *version) { continue; }
                let doc = self.document(version)?;
                if !self.can_delete_generated(&doc, translate)? {
                    return Err(error("没有可删除的生成结果；本地或云端字幕文件不在删除范围内"));
                }
                documents.push(doc);
            }
            self.db.with_conn(|conn| {
                let tx = conn.transaction()?;
                for doc in &mut documents {
                    if translate {
                        let trusted: std::collections::BTreeSet<_> = doc.original.iter()
                            .filter(|cue| doc.trusted_reference(cue.index).is_some()).map(|cue| cue.index).collect();
                        doc.translations.retain(|id, _| trusted.contains(id));
                        doc.translation_corrections.retain(|id, _| trusted.contains(id));
                        doc.translation_failures.clear();
                        doc.translation_key.clear();
                        crate::db::Database::write_subtitle_document(&tx, doc)?;
                    } else {
                        tx.execute("DELETE FROM subtitle_documents WHERE version_id=?1 AND source_key LIKE 'sensevoice:%'", [&doc.version_id])?;
                    }
                }
                tx.commit()?;
                Ok(())
            })?;
            // Explicit deletion must not be immediately undone by automatic work.
            *self.suppressed_generation.lock() = Some(generation);
            if let Some(doc) = documents.iter().find(|doc| current.as_deref() == Some(&doc.version_id)) {
                self.playback.publish_generated_subtitles(&doc.version_id, generation, &expected,
                    if translate { doc.rendered() } else { Vec::new() });
            }
            Ok(())
        })?;
        let mut task = self.task.lock();
        if task.cancel.is_none() && task.snapshot.version_id.as_ref().is_some_and(|id| versions.contains(id)) {
            task.snapshot = Default::default();
        }
        Ok(())
    }
    pub async fn readiness(&self, versions: &[String]) -> AppResult<Vec<SubtitleReadiness>> {
        if versions.is_empty() {
            return Ok(Vec::new());
        }
        validate_versions(versions)?;
        let engine = self.recognition.require_engine();
        let translation = self.recognition.require_translation().await;
        let library = crate::services::library::LibraryService::new(self.db.clone());
        Ok(versions
            .iter()
            .map(|version| {
                let doc = self.document(version).ok();
                let original = doc
                    .as_ref()
                    .map(|d| !d.original.is_empty())
                    .unwrap_or(false)
                    || library
                        .find_best_subtitle_for_version(version)
                        .ok()
                        .flatten()
                        .is_some();
                let source = self
                    .playback
                    .list_sources_for_version(version)
                    .map(|s| s.iter().any(|s| s.availability == "available"))
                    .unwrap_or(false);
                let recognition_error = engine
                    .as_ref()
                    .err()
                    .map(|e| e.message.clone())
                    .or_else(|| (!source).then(|| "音源不可用，请检查本地文件或在线音源".into()));
                let translation_error = translation
                    .as_ref()
                    .err()
                    .map(|e| e.message.clone())
                    .or_else(|| {
                        if original {
                            None
                        } else {
                            recognition_error
                                .clone()
                                .map(|reason| format!("尚无字幕原文，需要先识别：{reason}"))
                        }
                    });
                SubtitleReadiness {
                    version_id: version.clone(),
                    can_recognize: recognition_error.is_none(),
                    can_translate: translation_error.is_none(),
                    recognition_error,
                    translation_error,
                }
            })
            .collect())
    }
    pub fn start(&self, version_id: String, translate: bool) -> AppResult<SubtitleTaskSnapshot> {
        self.start_batch(vec![version_id], translate, false, false)
    }
    pub fn start_batch(
        &self,
        versions: Vec<String>,
        translate: bool,
        confirmed_chinese: bool,
        automatic: bool,
    ) -> AppResult<SubtitleTaskSnapshot> {
        self.start_batch_mode(versions, translate, confirmed_chinese, automatic, None)
    }
    pub fn start_batch_mode(
        &self,
        versions: Vec<String>,
        translate: bool,
        confirmed_chinese: bool,
        automatic: bool,
        restart: Option<bool>,
    ) -> AppResult<SubtitleTaskSnapshot> {
        validate_versions(&versions)?;
        let settings = self.recognition.settings()?;
        let mut unique = Vec::new();
        for version in versions {
            if !unique.contains(&version) {
                unique.push(version);
            }
        }
        for version in &unique {
            let doc = self.task_document(version, &settings, automatic)?;
            if (!translate && restart.is_some())
                || needs_recognition_with_context(
                    &doc,
                    translate,
                    automatic,
                    automatic && self.automatic_recognition_allowed(&doc, &settings)?,
                    analysis_context_ms(settings.preferences.quiet_speech),
                )
            {
                self.recognition
                    .require_engine_for(&settings.preferences.recognition_engine)?;
            }
            if translate && !confirmed_chinese && has_chinese(&self.external_subtitle_cues(version)?) {
                return Err(error("检测到可能已有中文字幕，请确认后再翻译"));
            }
        }
        let cancel = Arc::new(AtomicBool::new(false));
        let initial = {
            let mut task = self.task.lock();
            if task.cancel.is_some() {
                return Err(error("已有字幕任务正在运行，请等待或取消"));
            }
            // Automatic preparation may have awaited a service while the user
            // deleted results. Recheck under the same lock as explicit deletion.
            if automatic && *self.suppressed_generation.lock() == Some(self.playback.subtitle_context().0) {
                return Err(error("当前音轨的自动字幕任务已暂停"));
            }
            task.snapshot = SubtitleTaskSnapshot {
                automatic,
                version_id: unique.first().cloned(),
                state: "running".into(),
                phase: if translate {
                    "翻译字幕"
                } else {
                    "识别原文"
                }
                .into(),
                tracks_total: unique.len(),
                ..Default::default()
            };
            task.cancel = Some(cancel.clone());
            task.snapshot.clone()
        };
        let service = self.clone();
        tokio::spawn(async move {
            let mut failure = None;
            for (index, version) in unique.iter().enumerate() {
                if cancel.load(Ordering::SeqCst) {
                    break;
                }
                {
                    let mut task = service.task.lock();
                    task.snapshot.version_id = Some(version.clone());
                    task.snapshot.tracks_completed = index;
                    task.snapshot.completed = 0;
                    task.snapshot.total = 0;
                }
                let result = service
                    .run_version(
                        version,
                        translate,
                        settings.clone(),
                        automatic,
                        restart,
                        cancel.clone(),
                    )
                    .await;
                if let Err(e) = result {
                    failure = Some(e.message);
                    break;
                }
                service.task.lock().snapshot.tracks_completed = index + 1;
            }
            let mut task = service.task.lock();
            task.snapshot.state = if cancel.load(Ordering::SeqCst) {
                "cancelled"
            } else if failure.is_some() {
                "failed"
            } else {
                "completed"
            }
            .into();
            task.snapshot.error = if cancel.load(Ordering::SeqCst) {
                None
            } else {
                failure
            };
            task.cancel = None;
        });
        Ok(initial)
    }
    async fn run_version(
        &self,
        version: &str,
        translate: bool,
        settings: RecognitionSettings,
        automatic: bool,
        restart: Option<bool>,
        cancel: Arc<AtomicBool>,
    ) -> AppResult<()> {
        let mut doc = self.task_document(version, &settings, automatic)?;
        if !translate && !automatic {
            doc = self.manual_recognition_document(
                version,
                &doc,
                &settings,
                restart.unwrap_or(false),
            )?;
        }
        if doc.source_key.starts_with("sensevoice:")
            && self.db.audio_subtitle_key(version)? != doc.source_key
        {
            return Err(error("音源已变化，请重新载入音轨后识别"));
        }
        let mut settings = settings;
        if automatic {
            settings.full_text_review = false;
        }
        if translate
            && settings.full_text_review
            && !doc.recognition_complete
            && !doc.original.is_empty()
        {
            return Err(error(
                "全文校对需要完整原文，请先增量识别至完成，再点击翻译",
            ));
        }
        settings.preferences = settings
            .preferences
            .for_work(&self.subtitle_work_context(Some(version))?.work_id);
        let key = super::subtitle_preferences::translation_key(&settings);
        if translate && restart.unwrap_or(doc.translation_key != key) {
            doc.translations.clear();
            doc.translation_failures.clear();
        }
        if translate {
            doc.translation_key = key;
        }
        let (generation, current, cues, active_source) = self.playback.subtitle_context();
        let is_current = current.as_deref() == Some(version);
        let source = if is_current { active_source } else { None }.or(self
            .playback
            .list_sources_for_version(version)?
            .into_iter()
            .find(|s| s.availability == "available"));
        let duration = if is_current {
            self.playback.get_snapshot()?.duration_ms
        } else {
            None
        }
        .or(self.db.with_conn(|conn| {
            Ok(conn.query_row(
                "SELECT duration_ms FROM track_versions WHERE id=?1",
                [version],
                |r| r.get::<_, Option<i64>>(0),
            )?)
        })?)
        .unwrap_or(0);
        if !translate && !automatic {
            if cancel.load(Ordering::SeqCst) { return Err(error("已取消")); }
            if source.is_none() { return Err(error("音源尚未就绪，请开始播放后重试")); }
            self.db.clear_automatic_translations_for_version(version)?;
            doc.clear_automatic_translations();
        }
        self.execute(
            doc,
            generation,
            if is_current { cues } else { Vec::new() },
            source,
            duration,
            translate,
            settings,
            automatic,
            cancel,
        )
        .await
    }
    /// Host-owned follow-up: playback is the only trigger, never mounting a WebView.
    pub fn spawn_automatic(&self) {
        let service = self.clone();
        tauri::async_runtime::spawn(async move {
            let mut attempted = None;
            while !service.stopping.load(Ordering::SeqCst) {
                tokio::time::sleep(Duration::from_secs(1)).await;
                service.automatic_step(&mut attempted).await;
            }
        });
    }
    pub(crate) async fn automatic_step(&self, attempted: &mut Option<AutomaticAttempt>) {
        // Idle sessions need neither SQLite settings nor a full queue snapshot.
        let snapshot = self.playback.build_tick();
        if snapshot.state != "playing" || self.stopping.load(Ordering::SeqCst) {
            return;
        }
        let Ok(settings) = self.recognition.settings() else {
            return;
        };
        if !(settings.auto_recognition != AutoRecognitionMode::Off || settings.auto_translation)
            || self.stopping.load(Ordering::SeqCst)
        {
            return;
        }
        let Some(version) = self.playback.current_version_for_generation(snapshot.session_generation) else {
            return;
        };
        if *self.suppressed_generation.lock() == Some(snapshot.session_generation)
            || snapshot.state != "playing"
        {
            return;
        }
        let range = prefetch_chunks(snapshot.position_ms, settings.pre_read_seconds, usize::MAX);
        let key = (
            snapshot.session_generation,
            version.clone(),
            settings.clone(),
            range.start,
            range.end,
        );
        if attempted.as_ref() == Some(&key) || self.task.lock().cancel.is_some() {
            return;
        }
        *attempted = Some(key);
        // Reuse a known external subtitle before deciding that recognition is needed.
        if settings.auto_recognition != AutoRecognitionMode::All
            && self.prepare(std::slice::from_ref(&version)).await.is_err()
        {
            return;
        }
        let Ok(doc) = self.task_document(&version, &settings, true) else {
            return;
        };
        if settings.auto_recognition == AutoRecognitionMode::All && !doc.original.is_empty() {
            let (generation, _, mut expected, _) = self.playback.subtitle_context();
            if generation == snapshot.session_generation && expected != doc.rendered() {
                self.publish(&doc, generation, &mut expected);
            }
        }
        let mut effective = settings.clone();
        effective.full_text_review = false;
        if let Ok(context) = self.subtitle_work_context(Some(&version)) {
            effective.preferences = settings.preferences.for_work(&context.work_id);
        }
        let cache_matches =
            doc.translation_key == super::subtitle_preferences::translation_key(&effective);
        let translated = doc
            .original
            .iter()
            .filter(|c| in_prefetch(c, &range))
            .all(|c| {
                !doc.can_translate(c.index)
                    || (cache_matches && doc.translation_failures.contains_key(&c.index))
                    || doc.translation_corrections.contains_key(&c.index)
                    || (cache_matches && doc.translation_text(c.index).is_some())
            });
        let total = snapshot
            .duration_ms
            .map(|d| ((d.max(0) + CHUNK_MS - 1) / CHUNK_MS) as usize);
        let missing = !doc.recognition_complete
            && range
                .clone()
                .take_while(|i| total.map(|t| *i < t).unwrap_or(true))
                .any(|i| !doc.completed_chunks.contains(&i));
        let recognize = missing
            && self
                .automatic_recognition_allowed(&doc, &settings)
                .unwrap_or(false);
        let translate = settings.auto_translation
            && !has_chinese_translation_source(&doc)
            && (!translated || recognize)
            && self.recognition.translation_ready().await;
        if !translate && !recognize {
            return;
        }
        let (generation, current, _, _) = self.playback.subtitle_context();
        if generation != snapshot.session_generation || current.as_deref() != Some(&version) {
            return;
        }
        let _ = self.start_batch(vec![version], translate, true, true);
    }
    #[allow(clippy::too_many_arguments)]
    async fn execute(
        &self,
        mut doc: SubtitleDocument,
        generation: u64,
        mut expected: Vec<SubtitleCueDto>,
        source: Option<crate::domain::MediaSourceDto>,
        duration: i64,
        translate: bool,
        settings: RecognitionSettings,
        automatic: bool,
        cancel: Arc<AtomicBool>,
    ) -> AppResult<()> {
        doc.prepare_dialogue_only();
        super::subtitle_preferences::apply_rules(&mut doc, &settings.preferences);
        self.db.save_subtitle_document(&doc)?;
        self.publish(&doc, generation, &mut expected);
        if needs_recognition_with_context(
            &doc,
            translate,
            automatic,
            automatic && self.automatic_recognition_allowed(&doc, &settings)?,
            analysis_context_ms(settings.preferences.quiet_speech),
        ) {
            let source = source.ok_or_else(|| error("音源尚未就绪，请开始播放后重试"))?;
            let (uri, headers) = if let Some(path) = source.absolute_path {
                if path.ends_with(".part") || !std::path::Path::new(&path).is_file() {
                    return Err(error("本地音源不可用"));
                }
                (path, Vec::new())
            } else {
                let media = ProviderMediaRef {
                    provider: source.provider.unwrap_or_else(|| "asmr_one".into()),
                    source_work_id: source.source_work_id.unwrap_or_default(),
                    remote_file_id: source.remote_file_id.unwrap_or_default(),
                    remote_relative_path: source.remote_relative_path.unwrap_or_default(),
                };
                let resolved = tokio::select! { result=self.provider.resolve_media(&media,MediaPurpose::Play,false)=>result?, _=wait_cancel(&cancel)=>return Err(error("已取消")) };
                (resolved.url, resolved.headers)
            };
            let service = self.clone();
            let flag = cancel.clone();
            let preferences = settings.preferences.clone();
            (doc, expected) = tokio::task::spawn_blocking(move || {
                let duration = if duration > 0 {
                    duration
                } else {
                    probe_duration(&uri, &headers, &flag)?
                };
                service.recognize(
                    doc,
                    generation,
                    expected,
                    &uri,
                    &headers,
                    duration,
                    automatic,
                    &flag,
                    &preferences,
                )
            })
            .await
            .map_err(|_| error("识别任务异常结束，已保存片段可继续"))??;
        }
        if translate && !automatic && settings.full_text_review {
            if !doc.recognition_complete {
                return Err(error("请先完成识别，再进行全文校对"));
            }
            let transcript = super::recognition::full_text_translation::review_input(&doc);
            let pending: Vec<_> = doc
                .original
                .iter()
                .filter(|cue| doc.can_translate(cue.index))
                .filter(|cue| {
                    doc.translation_text(cue.index).is_none()
                        || !doc.model_reviews.contains_key(&cue.index)
                            && !doc.original_corrections.contains_key(&cue.index)
                            && !doc.rule_corrections.contains_key(&cue.index)
                })
                .map(|cue| cue.index)
                .collect();
            for targets in pending.chunks(LOCAL_BATCH_SIZE) {
                if cancel.load(Ordering::SeqCst) {
                    return Err(error("已取消"));
                }
                self.progress("全文校对并翻译", doc.translated_count(), doc.dialogue_count());
                let reviewed = tokio::select! {
                    result = self.recognition.review_full_text(&settings,&transcript,targets) => result,
                    _ = wait_cancel(&cancel) => return Err(error("已取消")),
                };
                if cancel.load(Ordering::SeqCst) {
                    return Err(error("已取消"));
                }
                let reviewed = match reviewed {
                    Ok(value) => value,
                    Err(err) => {
                        for id in targets {
                            doc.translation_failures.insert(*id, "校对请求失败".into());
                        }
                        self.db.save_subtitle_document(&doc)?;
                        return Err(err);
                    }
                };
                super::recognition::full_text_translation::apply_review(&mut doc, reviewed);
                for id in targets {
                    if doc.model_reviews.contains_key(id) {
                        doc.translation_failures.remove(id);
                    } else {
                        doc.translation_failures
                            .insert(*id, "校对返回缺失或无效".into());
                    }
                }
                doc.validate()?;
                self.db.save_subtitle_document(&doc)?;
                self.publish(&doc, generation, &mut expected);
                self.set_review_pending(&doc.version_id, doc.review_pending_count());
            }
            self.progress(
                &format!("校对已保存，{} 句待核对", doc.review_pending_count()),
                doc.translated_count(),
                doc.dialogue_count(),
            );
            let missing = pending
                .iter()
                .filter(|id| !doc.model_reviews.contains_key(id))
                .count();
            if missing > 0 {
                return Err(error(&format!("已保存有效校对，{missing} 句漏回或无效，可增量重试；待核对句请打开字幕编辑查看")));
            }
        } else if translate && !(automatic && has_chinese_translation_source(&doc)) {
            // Freeze the provider/model used for this job. Settings changes affect the next task.
            let mut attempted = std::collections::BTreeSet::new();
            for i in 0..doc.original.len() {
                if automatic {
                    let (now, current, _, _) = self.playback.subtitle_context();
                    let settings = self.recognition.settings()?;
                    if now != generation
                        || current.as_deref() != Some(&doc.version_id)
                        || !settings.auto_translation
                    {
                        cancel.store(true, Ordering::SeqCst);
                    }
                }
                if cancel.load(Ordering::SeqCst) {
                    return Err(error("已取消"));
                }
                let cue = &doc.original[i];
                let range = if automatic {
                    let snapshot = self.playback.get_snapshot()?;
                    let settings = self.recognition.settings()?;
                    Some(prefetch_chunks(
                        snapshot.position_ms,
                        settings.pre_read_seconds,
                        usize::MAX,
                    ))
                } else {
                    None
                };
                if range.as_ref().is_some_and(|range| !in_prefetch(cue, range)) {
                    continue;
                }
                if !doc.can_translate(cue.index)
                    || doc.translation_text(cue.index).is_some()
                    || attempted.contains(&cue.index)
                {
                    continue;
                }
                // Pure nonverbal words already express the sound; don't send them to a
                // language model that could expand a breath or laugh into dialogue.
                if let Some(sound) =
                    super::speech_segments::non_speech_translation(doc.original_text(cue))
                {
                    doc.translations.insert(cue.index, sound.to_owned());
                    self.db.save_subtitle_document(&doc)?;
                    self.publish(&doc, generation, &mut expected);
                    continue;
                }
                self.progress("翻译字幕", doc.translated_count(), doc.dialogue_count());
                let batch: Vec<_> = local_translation_batch(&doc, i, range.as_ref())
                    .into_iter()
                    .filter(|line| !attempted.contains(&line.id))
                    .collect();
                let targets = batch.iter().map(|line| line.id).collect::<Vec<_>>();
                let context = translation_context(&doc, &targets);
                let translations = tokio::select! {
                    result=async {
                        if settings.translation_provider == "ollama" {
                            self.recognition.translate_local_batch(&settings, &batch, &context).await
                        } else {
                            self.recognition.translate_deepseek_batch(&settings, &batch, &context).await
                        }
                    }=>result,
                    _=wait_cancel(&cancel)=>return Err(error("已取消")),
                };
                let translations = match translations {
                    Ok(value) => value,
                    Err(err) => {
                        for id in &targets {
                            doc.translation_failures.insert(*id, "翻译请求失败".into());
                        }
                        self.db.save_subtitle_document(&doc)?;
                        return Err(err);
                    }
                };
                attempted.extend(targets.iter().copied());
                if automatic {
                    let (now, current, _, _) = self.playback.subtitle_context();
                    if now != generation
                        || current.as_deref() != Some(&doc.version_id)
                        || !self.recognition.settings()?.auto_translation
                    {
                        cancel.store(true, Ordering::SeqCst);
                    }
                }
                if cancel.load(Ordering::SeqCst) {
                    return Err(error("已取消"));
                }
                // Only translations change. Original short cues, IDs and timing stay untouched.
                let translations = translations
                    .into_iter()
                    .filter(|(id, _)| {
                        !doc.translation_corrections.contains_key(id)
                            && doc.trusted_reference(*id).is_none()
                    })
                    .collect::<Vec<_>>();
                doc.translations.extend(translations);
                for id in targets {
                    if doc.translation_text(id).is_some() {
                        doc.translation_failures.remove(&id);
                    } else {
                        doc.translation_failures
                            .insert(id, "返回缺失或未通过结构与完整性检查".into());
                    }
                }
                self.db.save_subtitle_document(&doc)?;
                self.publish(&doc, generation, &mut expected);
            }
            self.progress("翻译字幕", doc.translated_count(), doc.dialogue_count());
            let remaining = attempted
                .iter()
                .filter(|id| doc.translation_text(**id).is_none())
                .count();
            if remaining > 0 {
                return Err(error(&format!("已保存可用译文，仍有 {remaining} 句未通过翻译检查；后续句子已继续处理，可增量重试待翻译句子")));
            }
        }
        self.publish(&doc, generation, &mut expected);
        self.set_review_pending(&doc.version_id, doc.review_pending_count());
        Ok(())
    }
    pub(super) fn set_review_pending(&self, version: &str, count: usize) {
        let mut task = self.task.lock();
        if task.snapshot.version_id.as_deref() == Some(version) {
            task.snapshot.review_pending = count;
        }
    }
    fn progress(&self, phase: &str, completed: usize, total: usize) {
        let mut task = self.task.lock();
        task.snapshot.phase = phase.into();
        task.snapshot.completed = completed;
        task.snapshot.total = total;
    }
    pub(super) fn publish(
        &self,
        doc: &SubtitleDocument,
        generation: u64,
        expected: &mut Vec<SubtitleCueDto>,
    ) {
        // Rule/editor callers may hold with_idle's task lock; publish must not reacquire it.
        let cues = doc.rendered();
        if self.playback.publish_generated_subtitles(
            &doc.version_id,
            generation,
            expected,
            cues.clone(),
        ) {
            *expected = cues;
        }
    }
    #[allow(clippy::too_many_arguments)]
    fn recognize(
        &self,
        mut doc: SubtitleDocument,
        generation: u64,
        mut expected: Vec<SubtitleCueDto>,
        uri: &str,
        headers: &[(String, String)],
        duration: i64,
        automatic: bool,
        cancel: &AtomicBool,
        preferences: &super::subtitle_preferences::SubtitlePreferences,
    ) -> AppResult<(SubtitleDocument, Vec<SubtitleCueDto>)> {
        let _model_use = self
            .recognition
            .model_access
            .try_read()
            .map_err(|_| error("正在删除模型，请稍后识别"))?;
        let total = if doc.reference_subtitles.is_empty() {
            ((duration + CHUNK_MS - 1) / CHUNK_MS) as usize
        } else {
            doc.reference_subtitles.len()
        };
        let directory = self
            .db
            .cache_dir
            .join("speech-analysis")
            .join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir_all(&directory).map_err(|_| error("无法创建分析音频缓存"))?;
        let _cleanup = AnalysisFiles(directory.clone());
        if cancel.load(Ordering::SeqCst) {
            return Err(error("已取消识别"));
        }
        self.recognition
            .require_engine_for(&preferences.recognition_engine)?;
        let mut sensevoice = if preferences.recognition_engine == "sensevoice" {
            Some(SenseVoice::new(
                &self.recognition.runtime_directory(),
                &self.recognition.model_directory(),
                &preferences.language,
            )?)
        } else {
            None
        };
        let mut whisper = if preferences.recognition_engine == "whisper" {
            Some(super::whisper_engine::Whisper::new(
                &self.recognition.whisper_directory(),
                &preferences.language,
                &preferences.recognition_prompt(),
            )?)
        } else {
            None
        };
        let mut fast = if ["faster_whisper", "kotoba_whisper"]
            .contains(&preferences.recognition_engine.as_str())
        {
            self.progress(
                "识别原文 · 初始化 Faster-Whisper",
                doc.completed_chunks.len(),
                total,
            );
            let engine = super::faster_whisper::FasterWhisper::new(
                &self
                    .recognition
                    .ct2_directory(&preferences.recognition_engine)?,
                &self.recognition.sensevoice_fp16_directory(),
                &preferences.language,
                &preferences.recognition_prompt(),
                cancel,
            )?;
            self.progress(
                &format!("识别原文 · {}", engine.device),
                doc.completed_chunks.len(),
                total,
            );
            Some(engine)
        } else {
            None
        };
        let mut gpu = if preferences.recognition_engine == "sensevoice_fp16" {
            self.progress(
                "识别原文 · 初始化 CUDA FP16",
                doc.completed_chunks.len(),
                total,
            );
            Some(super::sensevoice_gpu::GpuRecognizer::new(
                &self.recognition.sensevoice_fp16_directory(),
                &preferences.language,
                cancel,
            )?)
        } else {
            None
        };
        if !doc.reference_subtitles.is_empty() {
            if automatic {
                return Err(error("中文字幕辅助识别仅由手动识别启动"));
            }
            if preferences.language != "ja" {
                return Err(error("中文字幕辅助识别请将识别语言设为日语"));
            }
            return self.recognize_references(
                doc,
                generation,
                expected,
                uri,
                headers,
                duration,
                &directory,
                cancel,
                preferences,
                |audio, without_prompt| {
                    if let Some(engine) = whisper.as_mut() {
                        engine.transcribe_comparison(audio, cancel, without_prompt)
                    } else if let Some(engine) = fast.as_mut() {
                        engine.transcribe_comparison(audio, cancel, without_prompt)
                    } else if let Some(engine) = gpu.as_mut() {
                        engine.transcribe(audio, cancel)
                    } else {
                        sensevoice
                            .as_mut()
                            .expect("validated recognition engine")
                            .transcribe(audio)
                    }
                },
            );
        }
        let mut effective_language = preferences.language.clone();
        let mut dialogue_reviewer = super::dialogue_review::Reviewer::new(&self.recognition, &effective_language);
        dialogue_reviewer.set_context_ms(analysis_context_ms(preferences.quiet_speech));
        let mut cached_review_chunks = std::collections::BTreeMap::<usize, Vec<usize>>::new();
        if !automatic {
            for id in super::dialogue_review::cached_candidates_for_context(&doc, Some(analysis_context_ms(preferences.quiet_speech))) {
                if let Some(cue) = doc.original.iter().find(|c| c.index == id) {
                    cached_review_chunks.entry((cue.start_ms.max(0) / CHUNK_MS) as usize).or_default().push(id);
                }
            }
        }
        let mut check_language_context = preferences.language == "auto";
        let work_title = if check_language_context {
            self.subtitle_work_context(Some(&doc.version_id))
                .ok()
                .map(|context| {
                    self.db
                        .get_work_detail(&context.work_id)
                        .map(|work| work.title)
                        .unwrap_or(context.title)
                })
                .unwrap_or_default()
        } else {
            String::new()
        };
        while doc.completed_chunks.len() < total || !cached_review_chunks.is_empty() {
            let (current_gen, current, _, _) = self.playback.subtitle_context();
            if automatic
                && (current_gen != generation || current.as_deref() != Some(&doc.version_id))
            {
                cancel.store(true, Ordering::SeqCst);
            }
            if automatic {
                let settings = self.recognition.settings()?;
                if !self.automatic_recognition_allowed(&doc, &settings)? {
                    cancel.store(true, Ordering::SeqCst);
                }
            }
            if cancel.load(Ordering::SeqCst) {
                return Err(error("已取消识别"));
            }
            let position = if current.as_deref() == Some(&doc.version_id) {
                self.playback.get_snapshot()?.position_ms
            } else {
                0
            };
            let preferred = (position.max(0) / CHUNK_MS) as usize;
            let cached_chunk = cached_review_chunks.keys().next().copied();
            let chunk = if cached_chunk.is_some() {
                cached_chunk
            } else if automatic {
                let target = self.recognition.settings()?.pre_read_seconds;
                prefetch_chunks(position, target, total).find(|i| !doc.completed_chunks.contains(i))
            } else {
                next_chunk(total, preferred, &doc.completed_chunks)
            };
            let Some(chunk) = chunk else {
                break;
            };
            self.progress("识别原文", doc.completed_chunks.len(), total);
            let start = chunk as i64 * CHUNK_MS;
            let end = (start + CHUNK_MS).min(duration);
            let context_ms = analysis_context_ms(preferences.quiet_speech).max(2500);
            let decode_start = (start - context_ms).max(0);
            let cached_ids = cached_review_chunks.remove(&chunk);
            let decode_end = cached_ids.as_ref().map(|ids| doc.original.iter()
                .filter(|cue| ids.contains(&cue.index)).map(|cue| cue.end_ms).max().unwrap_or(end)).unwrap_or(end).max(end);
            let samples = decode_chunk(
                uri,
                headers,
                decode_start,
                (decode_end + context_ms).min(duration) - decode_start,
                &directory.join("chunk.pcm"),
                cancel,
            )?;
            if let Some(ids) = cached_ids {
                for id in ids {
                    if cancel.load(Ordering::SeqCst) { return Err(error("已取消识别")); }
                    let cue = super::dialogue_review::owned_cue(doc.original.iter().find(|cue| cue.index == id).expect("cached review cue"), &doc.asr_records[&id]);
                    self.progress("识别原文 · 复核已有字幕", doc.completed_chunks.len(), total);
                    let review = dialogue_reviewer.review(&cue, &samples, decode_start,
                        &preferences.recognition_engine, cancel, |audio| {
                            if let Some(e) = whisper.as_mut() { e.transcribe_comparison(audio, cancel, true) }
                            else if let Some(e) = fast.as_mut() { e.transcribe_comparison(audio, cancel, true) }
                            else if let Some(e) = gpu.as_mut() { e.transcribe(audio, cancel) }
                            else { sensevoice.as_mut().expect("validated engine").transcribe(audio) }
                        })?;
                    super::dialogue_review::apply(doc.asr_records.get_mut(&id).expect("cached review record"), review);
                    // Persist each completed review so cancellation resumes only missing work.
                    if self.db.audio_subtitle_key(&doc.version_id)? != doc.source_key {
                        return Err(error("音源已变化，请重新开始识别"));
                    }
                    self.db.save_subtitle_document(&doc)?;
                    self.publish(&doc, generation, &mut expected);
                }
                continue;
            }
            let mut cues = Vec::new();
            let core_begin = ((start - decode_start) * 16) as usize;
            let core_end = (((end - decode_start) * 16) as usize).min(samples.len());
            if core_begin >= core_end {
                return Err(error("未能读取待识别的音频片段，请重试"));
            }
            let regions = super::asr_quality::regions(&samples[core_begin..core_end]);
            if check_language_context && !regions.is_empty() {
                self.progress("识别原文 · 判断语言", doc.completed_chunks.len(), total);
                // Probe the whole owned block (up to 20 s) before decoding tiny
                // regions. This includes the context after an opening interjection.
                // The probe is only for language, never published as subtitle cues.
                let context = if let Some(engine) = whisper.as_mut() {
                    engine.transcribe(&samples[core_begin..core_end], cancel)?
                } else if let Some(engine) = fast.as_mut() {
                    engine.transcribe(&samples[core_begin..core_end], cancel)?
                } else if let Some(engine) = gpu.as_mut() {
                    engine.transcribe(&samples[core_begin..core_end], cancel)?
                } else {
                    sensevoice
                        .as_mut()
                        .expect("validated recognition engine")
                        .transcribe(&samples[core_begin..core_end])?
                };
                if cancel.load(Ordering::SeqCst) {
                    return Err(error("已取消识别"));
                }
                let language = super::speech_engine::contextual_language_with_title(
                    "auto",
                    &context.text,
                    &work_title,
                );
                if language != "auto" {
                    effective_language = language.to_owned();
                    if let Some(engine) = whisper.as_mut() {
                        engine.set_language(language)?;
                    } else if let Some(engine) = fast.as_mut() {
                        engine.set_language(language)?;
                    } else if gpu.is_some() {
                        drop(gpu.take());
                        gpu = Some(super::sensevoice_gpu::GpuRecognizer::new(
                            &self.recognition.sensevoice_fp16_directory(),
                            language,
                            cancel,
                        )?);
                    } else {
                        // Release the old native model before loading a replacement.
                        drop(sensevoice.take());
                        sensevoice = Some(SenseVoice::new(
                            &self.recognition.runtime_directory(),
                            &self.recognition.model_directory(),
                            language,
                        )?);
                    }
                }
                check_language_context = false;
            }
            for (begin, end) in regions {
                if automatic && self.playback.subtitle_context().0 != generation {
                    cancel.store(true, Ordering::SeqCst);
                }
                if cancel.load(Ordering::SeqCst) {
                    return Err(error("已取消识别"));
                }
                let owned_begin = core_begin + begin;
                let owned_end = core_begin + end;
                let prompt = preferences.recognition_prompt();
                let assessed = super::asr_quality::recognize(
                    super::asr_quality::Region {
                        samples: &samples,
                        offset_ms: decode_start,
                        begin: owned_begin,
                        end: owned_end,
                        model: &preferences.recognition_engine,
                        language: &effective_language,
                        prompt: &prompt,
                        context_samples: context_ms as usize * 16,
                    },
                    cancel,
                    |audio, without_prompt| {
                        if let Some(engine) = whisper.as_mut() {
                            engine.transcribe_comparison(audio, cancel, without_prompt)
                        } else if let Some(engine) = fast.as_mut() {
                            engine.transcribe_comparison(audio, cancel, without_prompt)
                        } else if let Some(engine) = gpu.as_mut() {
                            engine.transcribe(audio, cancel)
                        } else {
                            sensevoice
                                .as_mut()
                                .expect("validated recognition engine")
                                .transcribe(audio)
                        }
                    },
                )?;
                for assessed in assessed {
                    let index = chunk * 1000 + cues.len();
                    let cue = SubtitleCueDto {
                        index,
                        start_ms: assessed.start_ms,
                        end_ms: assessed.end_ms,
                        text: assessed.text,
                    };
                    doc.asr_records.insert(index, assessed.record);
                    cues.push(cue);
                }
            }
            if cancel.load(Ordering::SeqCst) {
                return Err(error("已取消识别"));
            }
            if self.db.audio_subtitle_key(&doc.version_id)? != doc.source_key {
                return Err(error("音源已变化，请重新开始识别"));
            }
            doc.original.extend(cues);
            doc.original.sort_by_key(|c| c.start_ms);
            deduplicate_audio_overlap(&mut doc);
            // Review final cue ownership, so overlap deduplication cannot discard
            // the new review or apply one window's decision to a different cue.
            for id in super::dialogue_review::cached_candidates_for_context(&doc, Some(analysis_context_ms(preferences.quiet_speech))).into_iter().filter(|id| id / 1000 == chunk) {
                self.progress("识别原文 · 复核非台词", doc.completed_chunks.len(), total);
                dialogue_reviewer.set_language(&effective_language);
                let cue = super::dialogue_review::owned_cue(doc.original.iter().find(|cue| cue.index == id).expect("review cue"), &doc.asr_records[&id]);
                let review = dialogue_reviewer.review(&cue, &samples, decode_start,
                    &preferences.recognition_engine, cancel, |audio| {
                        if let Some(e) = whisper.as_mut() { e.transcribe_comparison(audio, cancel, true) }
                        else if let Some(e) = fast.as_mut() { e.transcribe_comparison(audio, cancel, true) }
                        else if let Some(e) = gpu.as_mut() { e.transcribe(audio, cancel) }
                        else { sensevoice.as_mut().expect("validated engine").transcribe(audio) }
                    })?;
                super::dialogue_review::apply(doc.asr_records.get_mut(&id).expect("review record"), review);
            }
            doc.completed_chunks.insert(chunk);
            doc.recognition_complete = doc.completed_chunks.len() == total;

            super::subtitle_preferences::apply_rules(&mut doc, preferences);
            self.db.save_subtitle_document(&doc)?;
            if doc.recognition_complete {
                if let Ok(context) = self.subtitle_work_context(Some(&doc.version_id)) {
                    self.playback.notify_affinity(&context.work_id);
                }
            }
            self.publish(&doc, generation, &mut expected);
        }
        self.progress(
            if automatic {
                "当前预读已完成"
            } else {
                "识别原文"
            },
            doc.completed_chunks.len(),
            total,
        );
        Ok((doc, expected))
    }
    #[allow(clippy::too_many_arguments)]
    fn recognize_references(
        &self,
        mut doc: SubtitleDocument,
        generation: u64,
        mut expected: Vec<SubtitleCueDto>,
        uri: &str,
        headers: &[(String, String)],
        duration: i64,
        directory: &std::path::Path,
        cancel: &AtomicBool,
        preferences: &super::subtitle_preferences::SubtitlePreferences,
        mut transcribe: impl FnMut(&[f32], bool) -> AppResult<super::speech_segments::RecognitionResult>,
    ) -> AppResult<(SubtitleDocument, Vec<SubtitleCueDto>)> {
        if doc
            .reference_subtitles
            .iter()
            .any(|cue| cue.end_ms > duration)
        {
            return Err(error("中文字幕时间轴超出音轨，请先校准或选择匹配字幕"));
        }
        let total = doc.reference_subtitles.len();

        for index in 0..total {
            if doc.completed_chunks.contains(&index) {
                continue;
            }
            if cancel.load(Ordering::SeqCst) {
                return Err(error("已取消识别"));
            }
            self.progress(
                "按中文字幕时间段识别日语",
                doc.completed_chunks.len(),
                total,
            );
            let reference = doc.reference_subtitles[index].clone();
            let mut pieces = Vec::new();
            let mut records = Vec::new();
            let mut start = reference.start_ms;
            while start < reference.end_ms {
                let end = (start + 12_000).min(reference.end_ms);
                let context_ms = analysis_context_ms(preferences.quiet_speech);
                let decode_start = (start - context_ms).max(0);
                let samples = decode_chunk(
                    uri,
                    headers,
                    decode_start,
                    (end + context_ms).min(duration) - decode_start,
                    &directory.join("reference.pcm"),
                    cancel,
                )?;
                let prompt = preferences.recognition_prompt();
                let output = super::asr_quality::recognize(
                    super::asr_quality::Region {
                        samples: &samples,
                        offset_ms: decode_start,
                        begin: ((start - decode_start) * 16) as usize,
                        end: (((end - decode_start) * 16) as usize).min(samples.len()),
                        model: &preferences.recognition_engine,
                        language: "ja",
                        prompt: &prompt,
                        context_samples: context_ms as usize * 16,
                    },
                    cancel,
                    &mut transcribe,
                )?;
                for cue in output {
                    if !cue.text.trim().is_empty() {
                        pieces.push(cue.text);
                    }
                    records.push(cue.record);
                }
                start = end;
            }
            if cancel.load(Ordering::SeqCst) {
                return Err(error("已取消识别"));
            }
            if self.db.audio_subtitle_key(&doc.version_id)? != doc.source_key {
                return Err(error("音源已变化，请重新识别"));
            }
            let original = pieces.join(" ");
            apply_reference_result(&mut doc, index, &original);
            if let Some(mut record) = records.first().cloned() {
                record.pending_audio_review = records.iter().any(|r| r.pending_audio_review);
                record.confirmed_no_speech = records.iter().all(|r| r.confirmed_no_speech);
                record.note = if record.pending_audio_review {
                    "参考时间段中有未确认的识别结果；中文字幕已保留，请听音核对".into()
                } else {
                    record.note
                };
                record.attempts = records.into_iter().flat_map(|r| r.attempts).collect();
                doc.asr_records.insert(index, record);
            }
            doc.recognition_complete = doc.completed_chunks.len() == total;
            self.db.save_subtitle_document(&doc)?;
            self.publish(&doc, generation, &mut expected);
        }
        self.progress("中文字幕对应段落已识别", doc.completed_chunks.len(), total);

        Ok((doc, expected))
    }
}
fn chinese_reference_cues(cues: &[SubtitleCueDto]) -> AppResult<Vec<SubtitleCueDto>> {
    let mut output = Vec::new();
    for cue in cues {
        let chinese = cue
            .text
            .lines()
            .flat_map(|s| s.split(" // "))
            .filter(|text| {
                text.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c))
                    && !text.chars().any(|c| ('\u{3040}'..='\u{30ff}').contains(&c))
            })
            .collect::<Vec<_>>()
            .join("\n");
        if !chinese.is_empty() {
            output.push(SubtitleCueDto {
                index: output.len(),
                start_ms: cue.start_ms,
                end_ms: cue.end_ms,
                text: chinese,
            });
        }
    }
    output.sort_by_key(|cue| cue.start_ms);
    if output
        .iter()
        .any(|cue| cue.start_ms < 0 || cue.end_ms <= cue.start_ms)
        || output
            .windows(2)
            .any(|pair| pair[0].end_ms > pair[1].start_ms)
    {
        return Err(error(
            "需要匹配音轨且时间段不重叠的中文字幕（SRT/VTT/LRC）；无时间戳台本需要先做音频对齐",
        ));
    }
    Ok(output)
}
fn apply_reference_result(doc: &mut SubtitleDocument, index: usize, original: &str) {
    let reference = &doc.reference_subtitles[index];
    doc.original.push(SubtitleCueDto {
        index,
        start_ms: reference.start_ms,
        end_ms: reference.end_ms,
        text: original.to_owned(),
    });
    doc.original.sort_by_key(|cue| cue.start_ms);
    doc.translation_corrections
        .insert(index, reference.text.clone());
    doc.completed_chunks.insert(index);
}
/// Only overlapping observations of the same audio may be deduplicated.
/// Real repetitions at disjoint times retain their IDs and complete text.
fn deduplicate_audio_overlap(doc: &mut SubtitleDocument) {
    let mut remove = Vec::new();
    for pair in doc.original.windows(2) {
        let (a, b) = (&pair[0], &pair[1]);
        let overlap = a.end_ms.min(b.end_ms) - a.start_ms.max(b.start_ms);
        let longer = (a.end_ms - a.start_ms).max(b.end_ms - b.start_ms);
        if !a.text.is_empty()
            && a.text == b.text
            && overlap > 0
            && overlap * 10 >= longer * 8
            && doc.asr_records.get(&b.index).is_some_and(|r| r.dialogue_review.is_none())
            && [a.index, b.index].iter().all(|id| {
                doc.asr_records.get(id).is_some_and(|r| !r.human_confirmed)
                    && !doc.non_dialogue_cues.contains(id)
                    && !doc.original_corrections.contains_key(id)
                    && doc.translation_text(*id).is_none()
                    && !doc.model_corrections.contains_key(id)
            })
        {
            remove.push((a.index, b.index));
        }
    }
    for (keep, drop) in remove {
        if !doc.asr_records.contains_key(&keep) {
            continue;
        }
        if let Some(extra) = doc.asr_records.remove(&drop) {
            let retained = doc.asr_records.get_mut(&keep).expect("checked");
            retained.pending_audio_review |= extra.pending_audio_review;
            retained.attempts.extend(extra.attempts);
            if retained.pending_audio_review && retained.note.is_empty() {
                retained.note = extra.note;
            }
            doc.original.retain(|cue| cue.index != drop);
        }
    }
}
#[cfg(test)]
mod reference_tests {
    use super::*;
    #[test]
    fn t1513_chinese_reference_preserves_timing_and_chinese_without_back_translation() {
        let cues = vec![SubtitleCueDto {
            index: 25,
            start_ms: 1200,
            end_ms: 2800,
            text: "こんばんは\n晚上好".into(),
        }];
        let refs = chinese_reference_cues(&cues).unwrap();
        assert_eq!(refs[0].text, "晚上好");
        let mut doc = SubtitleDocument::new("v".into(), "sensevoice:test".into(), vec![], false);
        doc.reference_subtitles = refs;
        apply_reference_result(&mut doc, 0, "こんばんは");
        assert_eq!(doc.rendered()[0].text, "こんばんは\n晚上好");
        assert_eq!(
            (doc.original[0].start_ms, doc.original[0].end_ms),
            (1200, 2800)
        );
        assert!(doc.completed_chunks.contains(&0));
        doc.validate().unwrap();
        doc.translation_corrections.clear();
        doc.translations.clear();
        assert_eq!(doc.rendered()[0].text, "こんばんは\n晚上好");
        doc.translation_corrections.insert(0, "用户修正".into());
        assert_eq!(doc.rendered()[0].text, "こんばんは\n用户修正");
        doc.translation_corrections.clear();
        doc.original[0].start_ms += 1;
        assert!(
            doc.translation_text(0).is_none(),
            "Never attach a reference to a different interval"
        );
        let mut overlapping = cues.clone();
        overlapping.push(cues[0].clone());
        assert!(chinese_reference_cues(&overlapping).is_err());
        assert!(chinese_reference_cues(&[]).unwrap().is_empty());
    }
}
fn in_prefetch(cue: &SubtitleCueDto, range: &std::ops::Range<usize>) -> bool {
    cue.end_ms > range.start as i64 * CHUNK_MS && cue.start_ms < range.end as i64 * CHUNK_MS
}
// Read only already available original cues; never wait for or recognize more audio.
// Include neighbours of every target so skipped/manual cues and batch edges retain context.
fn translation_context(doc: &SubtitleDocument, targets: &[usize]) -> String {
    let mut positions = std::collections::BTreeSet::new();
    let target_positions: Vec<_> = doc
        .original
        .iter()
        .enumerate()
        .filter(|(_, cue)| targets.contains(&cue.index))
        .map(|(position, _)| position)
        .collect();
    for (position, cue) in doc.original.iter().enumerate() {
        if targets.contains(&cue.index) {
            for neighbour in position.saturating_sub(3)..(position + 4).min(doc.original.len()) {
                if !targets.contains(&doc.original[neighbour].index) {
                    positions.insert(neighbour);
                }
            }
        }
    }
    let mut remaining = 1800;
    let mut lines = Vec::new();
    for position in positions.into_iter().take(12) {
        if remaining == 0 {
            break;
        }
        let cue = &doc.original[position];
        if !doc.can_translate(cue.index) {
            continue;
        }
        let text: String = doc
            .dialogue_text(cue)
            .chars()
            .take(remaining.min(300))
            .collect();
        remaining -= text.chars().count();
        // IDs survive edits and need not increase with time. Explicitly mark the
        // neighbour's relation to a target instead of asking the model to sort IDs.
        let relation =
            if let Some(next) = target_positions.iter().find(|&&target| target > position) {
                format!("句子 {} 之前", doc.original[*next].index)
            } else if let Some(previous) = target_positions.last() {
                format!("句子 {} 之后", doc.original[*previous].index)
            } else {
                continue;
            };
        lines.push(format!("{relation} [{}] {}", cue.index, text));
    }
    lines.join("\n")
}
fn local_translation_batch(
    doc: &SubtitleDocument,
    start: usize,
    range: Option<&std::ops::Range<usize>>,
) -> Vec<TranslationLine> {
    let mut batch = Vec::new();
    let mut chars = 0;
    for cue in doc.original.iter().skip(start) {
        if !doc.can_translate(cue.index)
            || doc.translation_text(cue.index).is_some()
            || range.is_some_and(|range| !in_prefetch(cue, range))
            || super::speech_segments::is_non_speech_word(doc.original_text(cue))
        {
            continue;
        }
        let text = doc.dialogue_text(cue);
        let count = text.chars().count();
        // An existing long external cue is translated alone, never split or truncated.
        if batch.len() == LOCAL_BATCH_SIZE
            || (!batch.is_empty() && chars + count > LOCAL_BATCH_CHARS)
        {
            break;
        }
        batch.push(TranslationLine {
            id: cue.index,
            text: text.to_owned(),
        });
        chars += count;
    }
    batch
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubtitleReadiness {
    pub version_id: String,
    pub can_recognize: bool,
    pub can_translate: bool,
    pub recognition_error: Option<String>,
    pub translation_error: Option<String>,
}
// Manual translation consumes available original cues, even when recognition is
// incomplete; it must not require/restart an unrelated recognition model.
fn needs_recognition(
    doc: &SubtitleDocument,
    translate: bool,
    automatic: bool,
    auto_allowed: bool,
) -> bool {
    (!translate && !automatic && !super::dialogue_review::cached_candidates(doc).is_empty())
        || !doc.recognition_complete
        && if automatic {
            auto_allowed
        } else {
            !translate || doc.original.is_empty()
        }
}
fn needs_recognition_with_context(doc: &SubtitleDocument, translate: bool, automatic: bool, auto_allowed: bool, context_ms: i64) -> bool {
    needs_recognition(doc, translate, automatic, auto_allowed)
        || !translate && !automatic && !super::dialogue_review::cached_candidates_for_context(doc, Some(context_ms)).is_empty()
}
fn validate_versions(versions: &[String]) -> AppResult<()> {
    if versions.is_empty() || versions.len() > 1000 {
        return Err(error("请选择 1–1000 条音轨"));
    }
    Ok(())
}
fn has_chinese_translation_source(doc: &SubtitleDocument) -> bool {
    has_chinese(
        &doc.effective_original()
            .into_iter()
            .filter(|cue| doc.can_translate(cue.index))
            .collect::<Vec<_>>(),
    )
}
fn has_chinese(cues: &[SubtitleCueDto]) -> bool {
    cues.iter().any(|cue| {
        cue.text
            .lines()
            .flat_map(|line| line.split(" // "))
            .any(|line| {
                !super::speech_segments::is_non_speech_word(line)
                    && line
                        .chars()
                        .filter(|c| ('\u{4e00}'..='\u{9fff}').contains(c))
                        .count()
                        >= 2
                    && !line.chars().any(|c| {
                        ('\u{3040}'..='\u{30ff}').contains(&c)
                            || ('\u{ac00}'..='\u{d7af}').contains(&c)
                    })
            })
    })
}
fn next_chunk(
    total: usize,
    preferred: usize,
    done: &std::collections::BTreeSet<usize>,
) -> Option<usize> {
    let start = preferred.min(total.saturating_sub(1));
    (start..total).chain(0..start).find(|i| !done.contains(i))
}
async fn wait_cancel(cancel: &AtomicBool) {
    while !cancel.load(Ordering::SeqCst) {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
struct AnalysisFiles(std::path::PathBuf);
impl Drop for AnalysisFiles {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::{
        matchers::{method, path},
        Mock, MockServer, ResponseTemplate,
    };

    fn batch_document(count: usize) -> SubtitleDocument {
        let cues: Vec<_> = (0..count)
            .map(|i| SubtitleCueDto {
                index: i * 3 + 7,
                start_ms: i as i64 * 1000 + 123,
                end_ms: i as i64 * 1000 + 987,
                text: format!("こんばんは{i}"),
            })
            .collect();
        SubtitleDocument::new("v".into(), external_key(&cues), cues, true)
    }

    #[test]
    fn overlap_never_discards_a_saved_audio_review() {
        let mut doc = SubtitleDocument::new("v".into(), "sensevoice:test".into(), vec![
            SubtitleCueDto { index: 0, start_ms: 3000, end_ms: 4000, text: "ごめん".into() },
            SubtitleCueDto { index: 1, start_ms: 3001, end_ms: 4001, text: "ごめん".into() },
        ], false);
        for id in [0,1] { doc.asr_records.insert(id, super::super::dialogue_review::tests::record()); }
        doc.asr_records.get_mut(&1).unwrap().dialogue_review = Some(crate::domain::asr::DialogueReview {
            context_ms: 2500,
            revision: super::super::dialogue_review::REVIEW_REVISION,
            decision: crate::domain::asr::DialogueDecision::Review, note: "保留对照".into(), observations: vec![] });
        deduplicate_audio_overlap(&mut doc);
        assert_eq!(doc.original.len(), 2);
        assert!(doc.asr_records[&1].dialogue_review.is_some());
    }
    #[test]
    fn audio_overlap_dedup_preserves_raw_attempts_and_disjoint_repeated_dialogue() {
        let audio = vec![0.01; 16000];
        let result = super::super::asr_quality::recognize(
            super::super::asr_quality::Region {
                samples: &audio,
                offset_ms: 0,
                begin: 0,
                end: audio.len(),
                model: "fixture",
                language: "ja",
                prompt: "",
                context_samples: 32000,
            },
            &AtomicBool::new(false),
            |_, _| {
                Ok(super::super::speech_segments::RecognitionResult {
                    text: "ごめん".into(),
                    ..Default::default()
                })
            },
        )
        .unwrap();
        let mut doc = SubtitleDocument::new(
            "v".into(),
            "sensevoice:fixture".into(),
            vec![
                SubtitleCueDto {
                    index: 0,
                    start_ms: 0,
                    end_ms: 1000,
                    text: "ごめん".into(),
                },
                SubtitleCueDto {
                    index: 1,
                    start_ms: 50,
                    end_ms: 1050,
                    text: "ごめん".into(),
                },
                SubtitleCueDto {
                    index: 2,
                    start_ms: 8000,
                    end_ms: 9000,
                    text: "ごめん".into(),
                },
            ],
            true,
        );
        for id in 0..3 {
            doc.asr_records.insert(id, result[0].record.clone());
        }
        let before = doc.asr_records[&0].attempts.len();
        deduplicate_audio_overlap(&mut doc);
        assert_eq!(
            doc.original.iter().map(|c| c.index).collect::<Vec<_>>(),
            vec![0, 2]
        );
        assert_eq!(doc.asr_records[&0].attempts.len(), before * 2);
        assert_eq!(doc.original[1].text, "ごめん");
        doc.validate().unwrap();
        assert!(local_translation_batch(&doc, 0, None).is_empty()); // unresolved audio never becomes an empty translation retry
        doc.asr_records.get_mut(&0).unwrap().human_confirmed = true;
        assert_eq!(
            local_translation_batch(&doc, 0, None)
                .iter()
                .map(|l| l.id)
                .collect::<Vec<_>>(),
            vec![0]
        );
    }
    #[test]
    fn audio_overlap_dedup_never_overwrites_human_or_trusted_chinese() {
        let mut doc = batch_document(2);
        doc.original[1].text = doc.original[0].text.clone();
        doc.original[1].start_ms = doc.original[0].start_ms;
        doc.original[1].end_ms = doc.original[0].end_ms;
        let audio = vec![0.01; 16000];
        let result = super::super::asr_quality::recognize(
            super::super::asr_quality::Region {
                samples: &audio,
                offset_ms: 0,
                begin: 0,
                end: audio.len(),
                model: "fixture",
                language: "ja",
                prompt: "",
                context_samples: 32000,
            },
            &AtomicBool::new(false),
            |_, _| {
                Ok(super::super::speech_segments::RecognitionResult {
                    text: "こんばんは".into(),
                    ..Default::default()
                })
            },
        )
        .unwrap();
        for cue in &doc.original {
            doc.asr_records.insert(cue.index, result[0].record.clone());
        }
        doc.translation_corrections
            .insert(doc.original[1].index, "人工中文".into());
        let before = doc.revision();
        deduplicate_audio_overlap(&mut doc);
        assert_eq!(doc.revision(), before);
    }
    #[test]
    fn t1513_translation_context_includes_neighbours_and_corrected_originals_only() {
        let mut doc = batch_document(12);
        doc.original_corrections
            .insert(10, "修正后的相邻原文".into());
        doc.translation_corrections
            .insert(10, "不要作为原文上下文".into());
        doc.translations.insert(28, "不要使用自动译文".into());
        let context = translation_context(&doc, &[16, 19, 22, 25]);
        assert!(context.contains("[10] 修正后的相邻原文"));
        assert!(context.contains("[7] こんばんは0"));
        assert!(context.contains("[28] こんばんは7"));
        assert!(context.contains("[34] こんばんは9"));
        assert!(context.contains("句子 16 之前 [10]"));
        assert!(context.contains("句子 25 之后 [28]"));
        assert!(!context.contains("[37]"));
        assert!(!context.contains("[16]"));
        assert!(!context.contains("不要"));
        assert!(translation_context(&doc, &[7]).contains("[10]"));
        assert!(translation_context(&doc, &[40]).contains("[37]"));
        for cue in &mut doc.original {
            cue.text = "あ".repeat(1000);
        }
        let bounded = translation_context(&doc, &[7, 22, 40]);
        assert!(bounded.matches('あ').count() <= 1800);
        assert!(bounded.lines().count() <= 12);
    }

    #[tokio::test]
    async fn t1513_context_translation_retries_japanese_once_without_changing_cues() {
        let (_root, service) = batch_service();
        let server = MockServer::start().await;
        let attempts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let calls = attempts.clone();
        Mock::given(method("POST")).and(path("/api/chat")).respond_with(move |request: &wiremock::Request| {
            let body: serde_json::Value = request.body_json().unwrap();
            let input: serde_json::Value = serde_json::from_str(body["messages"][1]["content"].as_str().unwrap()).unwrap();
            assert!(input["context"].as_str().unwrap().contains("[10] こんばんは1"));
            if calls.fetch_add(1, Ordering::SeqCst) == 0 {
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"message":{"content":r#"{"translations":[{"id":7,"translation":"先に予告しないと"}]}"#}}))
            } else {
                assert!(body["messages"][0]["content"].as_str().unwrap().contains("上次结果仍含未翻译的日语"));
                batch_response(request)
            }
        }).expect(2).mount(&server).await;
        let mut doc = batch_document(3);
        doc.original[0].text = "ちゃーんと予告しとかないと".into();
        doc.translation_corrections.insert(10, "手工译文".into());
        doc.translation_corrections.insert(13, "".into());
        let original = doc.original.clone();
        let key = doc.source_key.clone();
        let settings = RecognitionSettings {
            ollama_url: server.uri(),
            ..Default::default()
        };
        service
            .execute(
                doc,
                0,
                vec![],
                None,
                0,
                true,
                settings.clone(),
                false,
                Arc::new(AtomicBool::new(false)),
            )
            .await
            .unwrap();
        let saved = service.db.subtitle_document("v", &key).unwrap().unwrap();
        assert_eq!(saved.original, original);
        assert_eq!(saved.translation_text(7), Some("译文7。"));
        assert_eq!(saved.translation_text(10), Some("手工译文"));
        assert_eq!(saved.translation_text(13), Some(""));
        server.verify().await;
        server.reset().await;
        Mock::given(method("POST")).and(path("/api/chat")).respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"message":{"content":r#"{"translations":[{"id":7,"translation":"先に予告しないと"}]}"#}}))).expect(2).mount(&server).await;
        let doc = batch_document(1);
        let key = doc.source_key.clone();
        service.db.save_subtitle_document(&doc).unwrap();
        assert!(service
            .execute(
                doc,
                0,
                vec![],
                None,
                0,
                true,
                settings,
                false,
                Arc::new(AtomicBool::new(false))
            )
            .await
            .is_err());
        assert_eq!(
            service
                .db
                .subtitle_document("v", &key)
                .unwrap()
                .unwrap()
                .translated_count(),
            0
        );
    }

    #[test]
    fn local_batch_bounds_skip_saved_manual_and_nonverbal_and_respect_prefetch() {
        let mut doc = batch_document(12);
        doc.translations.insert(7, "已保存".into());
        doc.translation_corrections.insert(10, "".into());
        doc.original[2].text = "呼".into();
        doc.original_corrections.insert(16, "修正后的原文".into());
        let batch = local_translation_batch(&doc, 0, None);
        assert_eq!(batch.len(), 6);
        assert_eq!(batch[0].id, 16);
        assert_eq!(batch[0].text, "修正后的原文");
        doc.original[4].text = "あ".repeat(600);
        assert_eq!(local_translation_batch(&doc, 3, None).len(), 1);
        assert_eq!(local_translation_batch(&doc, 4, None).len(), 1);
        doc.original[4].text = "あ".repeat(700);
        assert_eq!(
            local_translation_batch(&doc, 4, None)[0]
                .text
                .chars()
                .count(),
            700
        );
        let mut doc = batch_document(3);
        doc.original[1].start_ms = CHUNK_MS;
        doc.original[1].end_ms = CHUNK_MS + 1000;
        doc.original[2].start_ms = CHUNK_MS * 2;
        doc.original[2].end_ms = CHUNK_MS * 2 + 1000;
        let batch = local_translation_batch(&doc, 0, Some(&(1..2)));
        assert_eq!(batch.iter().map(|line| line.id).collect::<Vec<_>>(), [10]);
    }

    #[test]
    fn non_dialogue_is_skipped_by_normal_and_full_review_translation() {
        let mut doc = batch_document(3);
        doc.source_key = "sensevoice:test".into();
        doc.original[0].text = "ジュルジュル".into();
        doc.original[1].text = "ごめん".into();
        doc.original[2].text = "こんにちは フッフッフッ またね".into();
        let batch = local_translation_batch(&doc, 0, None);
        assert_eq!(batch.iter().map(|line| line.text.as_str()).collect::<Vec<_>>(), ["ごめん", "こんにちは またね"]);
        let review = super::super::recognition::full_text_translation::review_input(&doc);
        assert_eq!(review.len(), 2);
        assert!(translation_context(&doc, &[doc.original[2].index]).contains("ごめん"));
        assert!(!translation_context(&doc, &[doc.original[2].index]).contains("ジュル"));
        for line in batch { doc.translations.insert(line.id, "已翻译".into()); }
        assert!(local_translation_batch(&doc, 0, None).is_empty());
        assert_eq!(doc.translated_count(), doc.dialogue_count());
    }

    fn batch_service() -> (tempfile::TempDir, SubtitleTaskService) {
        let root = tempfile::tempdir().unwrap();
        let db =
            Database::open_and_migrate(&root.path().join("d"), &root.path().join("c")).unwrap();
        db.with_conn(|c| { c.execute_batch("INSERT INTO works(id,title,created_at,updated_at) VALUES('w','w','now','now'); INSERT INTO tracks(id,work_id,title,sort_key,created_at) VALUES('t','w','t','1','now'); INSERT INTO track_versions(id,track_id,created_at) VALUES('v','t','now');")?; Ok(()) }).unwrap();
        let provider = AsmrOneProvider::new(db.clone());
        let recognition = RecognitionService::new(db.clone(), root.path().join("d"), "batch-test");
        let playback = PlaybackService::new(db.clone(), provider.clone());
        (
            root,
            SubtitleTaskService::new(db, recognition, playback, provider),
        )
    }

    fn subtitle_source_service() -> (tempfile::TempDir, SubtitleTaskService) {
        let (root, service) = batch_service();
        let audio = root.path().join("synthetic.pcm");
        std::fs::write(&audio, [0_u8; 128]).unwrap();
        service.db.with_conn(|conn| {
            conn.execute("INSERT INTO media_sources(id,version_id,source_kind,absolute_path,format,availability,created_at,updated_at) VALUES('ref-audio','v','local_imported',?1,'pcm','available','now','now')", [audio.to_string_lossy().as_ref()])?;
            Ok(())
        }).unwrap();
        (root, service)
    }

    #[tokio::test]
    async fn manual_recognition_clears_machine_translations_only_when_started() {
        let (root, service) = subtitle_source_service();
        let file = root.path().join("japanese.srt");
        let content = "1\n00:00:01,000 --> 00:00:02,000\nおはよう\n";
        std::fs::write(&file, content).unwrap();
        service.import_local_subtitle("v", &file).unwrap();
        let mut external = service.document("v").unwrap();
        external.translations.insert(0, "旧文件译文".into());
        external.translation_corrections.insert(0, "手工文件译文".into());
        service.db.save_subtitle_document(&external).unwrap();
        let mut doc = service.recognition_document("v", true).unwrap();
        doc.original = vec![SubtitleCueDto { index: 7, start_ms: 0, end_ms: 1000, text: "おはよう".into() }];
        doc.recognition_complete = true;
        doc.completed_chunks.insert(0);
        doc.translations.insert(7, "旧自动译文".into());
        doc.translation_corrections.insert(7, "手工译文".into());
        doc.translation_failures.insert(7, "旧错误".into());
        doc.translation_key = "old".into();
        service.db.save_subtitle_document(&doc).unwrap();
        service.db.with_conn(|conn| {
            conn.execute("INSERT INTO track_versions(id,track_id,created_at) VALUES('other','t','now')", [])?;
            Ok(())
        }).unwrap();
        let mut other = doc.clone();
        other.version_id = "other".into();
        service.db.save_subtitle_document(&other).unwrap();
        let settings = RecognitionSettings { chinese_reference: false, auto_translation: false, ..Default::default() };
        assert!(service.run_version("v", false, settings.clone(), false, Some(false), Arc::new(AtomicBool::new(true))).await.is_err());
        assert_eq!(service.db.subtitle_document("v", &doc.source_key).unwrap().unwrap().translations, doc.translations);
        service.run_version("v", false, settings, false, Some(false), Arc::new(AtomicBool::new(false))).await.unwrap();
        for original in [&doc, &external] {
            let saved = service.db.subtitle_document("v", &original.source_key).unwrap().unwrap();
            assert!(saved.translations.is_empty() && saved.translation_failures.is_empty() && saved.translation_key.is_empty());
            assert_eq!(saved.translation_corrections, original.translation_corrections);
            assert_eq!(saved.original, original.original);
            assert_eq!(saved.completed_chunks, original.completed_chunks);
        }
        assert_eq!(service.db.subtitle_document("other", &doc.source_key).unwrap().unwrap().translations, other.translations);
        assert_eq!(std::fs::read_to_string(file).unwrap(), content);
        let mut reference = doc.clone();
        reference.original[0].index = 0;
        reference.reference_subtitles = vec![SubtitleCueDto { index: 0, text: "可信中文".into(), ..reference.original[0].clone() }];
        reference.clear_automatic_translations();
        assert_eq!(reference.translation_text(0), Some("可信中文"));
    }

    #[test]
    fn background_save_preserves_only_matching_confirmed_non_dialogue() {
        let (_root, service) = subtitle_source_service();
        let mut doc = service.recognition_document("v", true).unwrap();
        doc.original = vec![SubtitleCueDto { index: 7, start_ms: 0, end_ms: 1000, text: "ごめん".into() }];
        let mut stale = doc.clone();
        doc.non_dialogue_cues.insert(7);
        service.db.save_subtitle_document(&doc).unwrap();
        service.db.save_subtitle_document(&stale).unwrap();
        let saved = service.db.subtitle_document("v", &doc.source_key).unwrap().unwrap();
        assert!(saved.non_dialogue_cues.contains(&7));
        assert!(saved.rendered().is_empty());
        stale.original[0].start_ms = 5000; // Reused ID at a different occurrence is not the same evidence.
        stale.original[0].end_ms = 6000;
        service.db.save_subtitle_document(&stale).unwrap();
        let saved = service.db.subtitle_document("v", &doc.source_key).unwrap().unwrap();
        assert!(saved.non_dialogue_cues.is_empty());
        assert_eq!(saved.rendered()[0].text, "ごめん");
    }

    #[tokio::test]
    async fn generated_partial_translation_is_never_a_chinese_reference() {
        let (root, service) = subtitle_source_service();
        let file = root.path().join("japanese.srt");
        let content = "1\n00:00:01,000 --> 00:00:02,000\nおはよう\n\n2\n00:00:10,000 --> 00:00:11,000\nおやすみ\n";
        std::fs::write(&file, content).unwrap();
        service.import_local_subtitle("v", &file).unwrap();
        let mut displayed = service.document("v").unwrap();
        displayed.translations.insert(displayed.original[0].index, "早上好".into());
        service.db.save_subtitle_document(&displayed).unwrap();
        let settings = RecognitionSettings { chinese_reference: true, ..Default::default() };
        let restarted = service.manual_recognition_document("v", &displayed, &settings, true).unwrap();
        assert!(restarted.reference_subtitles.is_empty());
        assert!(!restarted.recognition_complete);
        assert!(service.prepare(&["v".into()]).await.unwrap().is_empty());
        let info = service.inspect(&["v".into()]).await.unwrap();
        assert_eq!(info["deletableRecognition"], serde_json::json!([]));
        assert_eq!(info["deletableTranslation"], serde_json::json!(["v"]));
        // Historical buggy reference caches must not be reused or copied on restart.
        let mut polluted = restarted.clone();
        polluted.reference_subtitles = chinese_reference_cues(&displayed.rendered()).unwrap();
        assert!(!polluted.reference_subtitles.is_empty());
        service.db.save_subtitle_document(&polluted).unwrap();
        assert!(service.manual_recognition_document("v", &displayed, &settings, false).is_err());
        assert!(service.manual_recognition_document("v", &displayed, &settings, true).unwrap().reference_subtitles.is_empty());
        service.delete_generated(&["v".into()], true).unwrap();
        let cleared = service.document("v").unwrap();
        assert_eq!(cleared.original, displayed.original);
        assert!(cleared.translations.is_empty());
        assert_eq!(std::fs::read_to_string(file).unwrap(), content);
        assert!(service.delete_generated(&["v".into()], false).is_err());
    }

    #[tokio::test]
    async fn deleting_generated_subtitles_preserves_sources_and_rejects_running_tasks() {
        let (_root, service) = subtitle_source_service();
        let mut doc = service.recognition_document("v", true).unwrap();
        doc.original = vec![SubtitleCueDto { index: 7, start_ms: 0, end_ms: 1000, text: "おはよう".into() }];
        doc.completed_chunks.insert(0);
        doc.translations.insert(7, "早上好".into());
        doc.translation_corrections.insert(7, "早安".into());
        service.db.save_subtitle_document(&doc).unwrap();
        let info = service.inspect(&["v".into()]).await.unwrap();
        assert_eq!(info["chinese"], serde_json::json!([]));
        assert_eq!(info["deletableRecognition"], serde_json::json!(["v"]));
        service.task.lock().cancel = Some(Arc::new(AtomicBool::new(false)));
        assert!(service.delete_generated(&["v".into()], false).is_err());
        service.task.lock().cancel = None;
        service.delete_generated(&["v".into()], true).unwrap();
        let cleared = service.document("v").unwrap();
        assert_eq!(cleared.original, doc.original);
        assert!(cleared.translation_corrections.is_empty() && cleared.translations.is_empty());
        service.delete_generated(&["v".into()], false).unwrap();
        assert!(service.db.subtitle_document("v", &doc.source_key).unwrap().is_none());
        assert!(service.document("v").unwrap().original.is_empty());
        assert_eq!(*service.suppressed_generation.lock(), Some(service.playback.subtitle_context().0));
    }

    #[tokio::test]
    async fn file_chinese_and_trusted_references_have_no_delete_action() {
        let (root, service) = subtitle_source_service();
        let file = root.path().join("chinese.srt");
        let content = "1\n00:00:01,000 --> 00:00:02,000\n晚上好\n";
        std::fs::write(&file, content).unwrap();
        service.import_local_subtitle("v", &file).unwrap();
        let settings = RecognitionSettings { chinese_reference: true, ..Default::default() };
        let displayed = service.document("v").unwrap();
        let mut doc = service.manual_recognition_document("v", &displayed, &settings, true).unwrap();
        apply_reference_result(&mut doc, 0, "こんばんは");
        assert!(!service.can_delete_generated(&doc, false).unwrap());
        assert!(!service.can_delete_generated(&doc, true).unwrap());
        for translate in [false, true] { assert!(service.delete_generated(&["v".into()], translate).is_err()); }
        let info = service.inspect(&["v".into()]).await.unwrap();
        assert_eq!(info["chinese"], serde_json::json!(["v"]));
        assert_eq!(std::fs::read_to_string(file).unwrap(), content);
    }

    #[test]
    fn completed_asr_cache_is_reviewed_only_by_manual_recognition_and_only_once() {
        let cue = SubtitleCueDto { index: 7, start_ms: 3000, end_ms: 4000, text: "ごめん".into() };
        let mut doc = SubtitleDocument::new("v".into(), "sensevoice:test".into(), vec![cue], true);
        doc.completed_chunks.insert(0);
        doc.asr_records.insert(7, super::super::dialogue_review::tests::record());
        assert!(needs_recognition(&doc, false, false, false));
        assert!(!needs_recognition(&doc, true, false, false));
        assert!(!needs_recognition(&doc, false, true, true));
        doc.asr_records.get_mut(&7).unwrap().dialogue_review = Some(crate::domain::asr::DialogueReview {
            context_ms: 2500,
            revision: super::super::dialogue_review::REVIEW_REVISION,
            decision: crate::domain::asr::DialogueDecision::Review, note: "保留核对".into(), observations: vec![] });
        assert!(!needs_recognition(&doc, false, false, false));
        assert_eq!(doc.completed_chunks.len(), 1);
        assert!(doc.recognition_complete);
    }
    #[test]
    fn t1513_optional_chinese_reference_falls_back_and_restores_bilingual() {
        let (root, service) = batch_service();
        let audio = root.path().join("synthetic.pcm");
        std::fs::write(&audio, [0_u8; 128]).unwrap();
        service.db.with_conn(|conn| {
            conn.execute("INSERT INTO media_sources(id,version_id,source_kind,absolute_path,format,availability,created_at,updated_at) VALUES('ref-audio','v','local_imported',?1,'pcm','available','now','now')",[audio.to_string_lossy().as_ref()])?;
            Ok(())
        }).unwrap();
        let settings = RecognitionSettings {
            chinese_reference: true,
            ..Default::default()
        };
        let empty = service.document("v").unwrap();
        let mut ordinary = service
            .manual_recognition_document("v", &empty, &settings, false)
            .unwrap();
        assert!(ordinary.reference_subtitles.is_empty());
        assert!(needs_recognition(&ordinary, false, false, false));
        ordinary.original.push(SubtitleCueDto {
            index: 0,
            start_ms: 100,
            end_ms: 500,
            text: "こんばんは".into(),
        });
        ordinary.completed_chunks.insert(0);
        service.db.save_subtitle_document(&ordinary).unwrap();
        let resumed = service
            .manual_recognition_document("v", &ordinary, &settings, false)
            .unwrap();
        assert_eq!(resumed.original, ordinary.original);
        assert_eq!(resumed.completed_chunks, ordinary.completed_chunks);
        assert!(resumed.reference_subtitles.is_empty());
        let file = root.path().join("chinese.srt");
        std::fs::write(&file, "1\n00:00:01,000 --> 00:00:02,000\n晚上好\n").unwrap();
        service.import_local_subtitle("v", &file).unwrap();
        let displayed = service.document("v").unwrap();
        assert!(service
            .manual_recognition_document("v", &displayed, &settings, false)
            .is_err());
        let mut referenced = service
            .manual_recognition_document("v", &displayed, &settings, true)
            .unwrap();
        assert_eq!(referenced.reference_subtitles.len(), 1);
        apply_reference_result(&mut referenced, 0, "こんばんは");
        referenced.recognition_complete = true;
        // References survive even when a historical cache is missing its translation map.
        referenced.translation_corrections.clear();
        service.db.save_subtitle_document(&referenced).unwrap();
        let reloaded = service.db.subtitle_overlay("v", displayed.original.clone());
        assert_eq!(reloaded[0].text, "こんばんは\n晚上好");
        let document = service
            .db
            .document_for_display("v", &referenced.original)
            .unwrap()
            .unwrap();
        assert_eq!(document.translation_text(0), Some("晚上好"));
        let mut different = displayed.original;
        different[0].text = "其他字幕".into();
        assert_eq!(
            service.db.subtitle_overlay("v", different.clone()),
            different
        );
        assert!(std::fs::read_to_string(file).unwrap().contains("晚上好"));
    }

    #[tokio::test]
    async fn t1513_full_review_task_saves_corrections_and_incremental_skips_reviewed_lines() {
        let (_root, service) = batch_service();
        let server = MockServer::start().await;
        Mock::given(method("POST")).and(path("/api/chat")).respond_with(|request:&wiremock::Request| {
            let body:serde_json::Value=request.body_json().unwrap();
            let input:serde_json::Value=serde_json::from_str(body["messages"][1]["content"].as_str().unwrap()).unwrap();
            assert_eq!(input["transcript"].as_array().unwrap().len(),2);
            let subtitles:Vec<_>=input["targets"].as_array().unwrap().iter().map(|id|serde_json::json!({"id":id,"original":input["transcript"].as_array().unwrap().iter().find(|line| line["id"]==*id).unwrap()["original"],"parts":[{"source":input["transcript"].as_array().unwrap().iter().find(|line| line["id"]==*id).unwrap()["original"],"translation":"晚上好"}]})).collect();
            ResponseTemplate::new(200).set_body_json(serde_json::json!({"done":true,"done_reason":"stop","message":{"content":serde_json::json!({"subtitles":subtitles}).to_string()}}))
        }).expect(1).mount(&server).await;
        let settings = RecognitionSettings {
            ollama_url: server.uri(),
            full_text_review: true,
            ..Default::default()
        };
        let mut doc = batch_document(2);
        doc.recognition_complete = true;
        let raw = doc.original.clone();
        let key = doc.source_key.clone();
        service
            .execute(
                doc,
                0,
                vec![],
                None,
                10_000,
                true,
                settings.clone(),
                false,
                Arc::new(AtomicBool::new(false)),
            )
            .await
            .unwrap();
        let saved = service.db.subtitle_document("v", &key).unwrap().unwrap();
        assert_eq!(saved.original, raw);
        assert_eq!(saved.model_corrections.len(), 2);
        assert_eq!(saved.translated_count(), 2);
        service
            .execute(
                saved,
                0,
                vec![],
                None,
                10_000,
                true,
                settings,
                false,
                Arc::new(AtomicBool::new(false)),
            )
            .await
            .unwrap();
    }
    #[tokio::test]
    async fn t1513_partial_original_translates_without_recognition_model() {
        let (root, service) = batch_service();
        let audio = root.path().join("synthetic.pcm");
        std::fs::write(&audio, [0_u8; 128]).unwrap();
        service.db.with_conn(|conn| {
            conn.execute("INSERT INTO media_sources(id,version_id,source_kind,absolute_path,format,availability,created_at,updated_at) VALUES('s','v','local_imported',?1,'pcm','available','now','now')",[audio.to_string_lossy().as_ref()])?; Ok(())
        }).unwrap();
        let mut doc = service.recognition_document("v", false).unwrap();
        doc.original = vec![SubtitleCueDto {
            index: 0,
            start_ms: 100,
            end_ms: 800,
            text: "おはよう".into(),
        }];
        doc.completed_chunks.insert(0);
        service.db.save_subtitle_document(&doc).unwrap();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/tags"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"models":[{"name":"qwen3:4b"}]})),
            )
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .respond_with(batch_response)
            .expect(1)
            .mount(&server)
            .await;
        service
            .recognition
            .save_settings(RecognitionSettings {
                ollama_url: server.uri(),
                ..Default::default()
            })
            .unwrap();
        assert!(service.recognition.require_engine().is_err());
        let readiness = service.readiness(&["v".into()]).await.unwrap();
        assert!(!readiness[0].can_recognize);
        assert!(
            readiness[0].can_translate,
            "{:?}",
            readiness[0].translation_error
        );
        service
            .start_batch_mode(vec!["v".into()], true, true, false, Some(false))
            .unwrap();
        for _ in 0..100 {
            if service.snapshot().state != "running" {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(
            service.snapshot().state,
            "completed",
            "{:?}",
            service.snapshot().error
        );
        let saved = service.document("v").unwrap();
        assert_eq!(saved.original, doc.original);
        assert!(!saved.recognition_complete);
        assert_eq!(saved.completed_chunks, doc.completed_chunks);
        assert_eq!(saved.translated_count(), 1);
        server.reset().await;
        Mock::given(method("GET"))
            .and(path("/api/tags"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"models":[]})),
            )
            .mount(&server)
            .await;
        let readiness = service.readiness(&["v".into()]).await.unwrap();
        assert!(readiness[0]
            .translation_error
            .as_deref()
            .unwrap()
            .contains("qwen3:4b"));
        server.reset().await;
        Mock::given(method("GET"))
            .and(path("/api/tags"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&server)
            .await;
        assert!(service.readiness(&["v".into()]).await.unwrap()[0]
            .translation_error
            .as_deref()
            .unwrap()
            .contains("503"));
    }

    #[tokio::test]
    async fn t1513_local_import_and_explicit_incremental_restart_preserve_files_and_manual_translations(
    ) {
        let (root, service) = batch_service();
        let file = root.path().join("picked.srt");
        let content = "1\n00:00:01,000 --> 00:00:02,000\nおはよう\n\n2\n00:00:03,000 --> 00:00:04,000\nこんばんは\n\n3\n00:00:05,000 --> 00:00:06,000\nありがとう\n";
        std::fs::write(&file, content).unwrap();
        service.import_local_subtitle("v", &file).unwrap();
        let library = crate::services::library::LibraryService::new(service.db.clone());
        assert_eq!(
            library
                .find_best_subtitle_for_version("v")
                .unwrap()
                .unwrap()
                .id,
            "manual-subtitle:v"
        );
        let mut doc = service.document("v").unwrap();
        assert_eq!(doc.original.len(), 3);
        let original = doc.original.clone();
        let ids: Vec<_> = original.iter().map(|cue| cue.index).collect();
        doc.translations.insert(ids[0], "保留旧自动译文".into());
        doc.translation_corrections
            .insert(ids[1], "保留手工译文".into());
        doc.translation_key = "old-settings".into();
        service.db.save_subtitle_document(&doc).unwrap();
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .respond_with(batch_response)
            .expect(2)
            .mount(&server)
            .await;
        let settings = RecognitionSettings {
            ollama_url: server.uri(),
            ..Default::default()
        };
        service
            .run_version(
                "v",
                true,
                settings.clone(),
                false,
                Some(false),
                Arc::new(AtomicBool::new(false)),
            )
            .await
            .unwrap();
        let saved = service.document("v").unwrap();
        assert_eq!(saved.translation_text(ids[0]), Some("保留旧自动译文"));
        assert_eq!(saved.translation_text(ids[1]), Some("保留手工译文"));
        assert_eq!(saved.translated_count(), 3);
        service
            .run_version(
                "v",
                true,
                settings,
                false,
                Some(true),
                Arc::new(AtomicBool::new(false)),
            )
            .await
            .unwrap();
        let saved = service.document("v").unwrap();
        assert_eq!(
            saved.translation_text(ids[0]),
            Some(format!("译文{}。", ids[0]).as_str())
        );
        assert_eq!(saved.translation_text(ids[1]), Some("保留手工译文"));
        assert_eq!(saved.original, original);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), content);
        let bad = root.path().join("bad.srt");
        std::fs::write(&bad, "no timestamp").unwrap();
        assert!(service.import_local_subtitle("v", &bad).is_err());
        assert_eq!(
            library
                .find_best_subtitle_for_version("v")
                .unwrap()
                .unwrap()
                .absolute_path,
            Some(file.canonicalize().unwrap().to_string_lossy().into_owned())
        );
        let audio_file = root.path().join("synthetic.pcm");
        std::fs::write(&audio_file, [0_u8; 128]).unwrap();
        service.db.with_conn(|conn| {
            conn.execute("INSERT INTO media_sources(id,version_id,source_kind,absolute_path,format,availability,created_at,updated_at) VALUES('test-audio','v','local_imported',?1,'pcm','available','now','now')", [audio_file.to_string_lossy().as_ref()])?;
            Ok(())
        }).unwrap();
        let mut audio = service.recognition_document("v", false).unwrap();
        audio.original = original;
        audio.completed_chunks.insert(0);
        audio.original_corrections.insert(ids[0], "识别修正".into());
        audio.translations.insert(ids[0], "旧译文".into());
        service.db.save_subtitle_document(&audio).unwrap();
        assert_eq!(
            service
                .recognition_document("v", false)
                .unwrap()
                .completed_chunks
                .len(),
            1
        );
        let restarted = service.recognition_document("v", true).unwrap();
        assert!(
            restarted.original.is_empty()
                && restarted.completed_chunks.is_empty()
                && restarted.translations.is_empty()
                && restarted.original_corrections.is_empty()
        );
        assert!(!restarted.recognition_complete);
        assert_eq!(std::fs::read_to_string(file).unwrap(), content);
    }

    fn batch_response(request: &wiremock::Request) -> ResponseTemplate {
        let body: serde_json::Value = request.body_json().unwrap();
        assert_eq!(body["think"], false);
        assert_eq!(body["format"]["type"], "object");
        let input: serde_json::Value =
            serde_json::from_str(body["messages"][1]["content"].as_str().unwrap()).unwrap();
        let lines = input["sentences"].as_array().unwrap();
        // Reversed provider order must still map to the correct original cue IDs.
        let translated: Vec<_> = lines.iter().rev().map(|line| serde_json::json!({"id":line["id"], "parts":[{"source":line["text"],"translation":format!("译文{}。", line["id"])}]})).collect();
        ResponseTemplate::new(200).set_body_json(serde_json::json!({"message":{"content":serde_json::json!({"translations":translated}).to_string()}}))
    }

    #[tokio::test]
    async fn t1513_bad_translation_line_does_not_block_later_cues_and_incremental_only_retries_missing(
    ) {
        let (_root, service) = batch_service();
        let server = MockServer::start().await;
        Mock::given(method("POST")).and(path("/api/chat")).respond_with(|request: &wiremock::Request| {
            let body: serde_json::Value = request.body_json().unwrap();
            let input: serde_json::Value = serde_json::from_str(body["messages"][1]["content"].as_str().unwrap()).unwrap();
            let lines: Vec<_> = input["sentences"].as_array().unwrap().iter().map(|line| {
                let id = line["id"].as_u64().unwrap();
                serde_json::json!({"id":id,"parts":[{"source":line["text"],"translation":if id == 10 {line["text"].as_str().unwrap().to_owned()} else {format!("译文{id}")}}]})
            }).collect();
            ResponseTemplate::new(200).set_body_json(serde_json::json!({"message":{"content":serde_json::json!({"translations":lines}).to_string()}}))
        }).expect(3).mount(&server).await;
        let mut doc = batch_document(9);
        doc.original[2].text = "ジュルジュル".into();
        let original = doc.original.clone();
        let key = doc.source_key.clone();
        let settings = RecognitionSettings {
            ollama_url: server.uri(),
            ..Default::default()
        };
        let err = service
            .execute(
                doc,
                0,
                vec![],
                None,
                0,
                true,
                settings.clone(),
                false,
                Arc::new(AtomicBool::new(false)),
            )
            .await
            .unwrap_err();
        assert!(err.message.contains("仍有 1 句"));
        let saved = service.db.subtitle_document("v", &key).unwrap().unwrap();
        assert_eq!(saved.translated_count(), 8);
        assert_eq!(saved.original, original);
        assert_eq!(saved.translation_text(13), Some("啾噜啾噜"));
        assert!(saved
            .translation_text(original.last().unwrap().index)
            .is_some());
        server.verify().await;
        server.reset().await;
        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .respond_with(|request: &wiremock::Request| {
                let body: serde_json::Value = request.body_json().unwrap();
                let input: serde_json::Value =
                    serde_json::from_str(body["messages"][1]["content"].as_str().unwrap()).unwrap();
                assert_eq!(input["sentences"].as_array().unwrap().len(), 1);
                assert_eq!(input["sentences"][0]["id"], 10);
                batch_response(request)
            })
            .expect(1)
            .mount(&server)
            .await;
        service
            .execute(
                saved,
                0,
                vec![],
                None,
                0,
                true,
                settings,
                false,
                Arc::new(AtomicBool::new(false)),
            )
            .await
            .unwrap();
        assert_eq!(
            service
                .db
                .subtitle_document("v", &key)
                .unwrap()
                .unwrap()
                .translated_count(),
            9
        );
    }

    #[tokio::test]
    async fn local_batch_requests_preserve_timing_and_resume_without_retranslating() {
        let (_root, service) = batch_service();
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .respond_with(batch_response)
            .expect(2)
            .mount(&server)
            .await;
        let mut doc = batch_document(15);
        doc.translations.insert(7, "已有译文".into());
        doc.translation_corrections.insert(10, "手工译文".into());
        doc.original[2].text = "呼".into();
        let mut settings = RecognitionSettings {
            ollama_url: server.uri(),
            ..Default::default()
        };
        settings.translation_options.temperature = 0.4;
        settings.translation_options.max_tokens = 4096;
        settings.preferences.translation_punctuation = "minimal".into();
        settings
            .preferences
            .glossary
            .push(super::super::subtitle_preferences::Term {
                source: "こんばんは".into(),
                target: "晚上好".into(),
                ..Default::default()
            });
        let original = doc.original.clone();
        let key = doc.source_key.clone();
        service
            .execute(
                doc,
                0,
                vec![],
                None,
                0,
                true,
                settings.clone(),
                false,
                Arc::new(AtomicBool::new(false)),
            )
            .await
            .unwrap();
        let saved = service.db.subtitle_document("v", &key).unwrap().unwrap();
        assert_eq!(saved.original, original);
        assert_eq!(saved.translation_text(7), Some("已有译文"));
        assert_eq!(saved.translation_text(10), Some("手工译文"));
        assert_eq!(saved.translation_text(13), Some("呼"));
        for cue in saved.original.iter().skip(3) {
            assert_eq!(
                saved.translation_text(cue.index),
                Some(format!("译文{}", cue.index).as_str())
            );
        }
        assert_eq!(
            saved
                .rendered()
                .iter()
                .map(|c| (c.index, c.start_ms, c.end_ms))
                .collect::<Vec<_>>(),
            original
                .iter()
                .map(|c| (c.index, c.start_ms, c.end_ms))
                .collect::<Vec<_>>()
        );
        assert_eq!(saved.translated_count(), 15);
        service
            .execute(
                saved,
                0,
                vec![],
                None,
                0,
                true,
                settings,
                false,
                Arc::new(AtomicBool::new(false)),
            )
            .await
            .unwrap();
        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 2); // 12 pending lines in two requests, no model pull.
        for request in requests {
            let body: serde_json::Value = request.body_json().unwrap();
            assert_eq!(body["options"]["temperature"], 0.4);
            assert_eq!(body["options"]["num_predict"], 4096);
            let input: serde_json::Value =
                serde_json::from_str(body["messages"][1]["content"].as_str().unwrap()).unwrap();
            assert_eq!(input["sentences"].as_array().unwrap().len(), 6);
            assert_eq!(input["glossary"][0]["target"], "晚上好");
        }
    }

    #[tokio::test]
    async fn local_batch_cancel_and_malformed_response_keep_saved_progress() {
        let (_root, service) = batch_service();
        let server = MockServer::start().await;
        let doc = batch_document(8);
        let key = doc.source_key.clone();
        let settings = RecognitionSettings {
            ollama_url: server.uri(),
            ..Default::default()
        };
        // First batch succeeds; the second cannot be mapped. Resume must start at the last two.
        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .respond_with(|request: &wiremock::Request| {
                let body: serde_json::Value = request.body_json().unwrap();
                let input: serde_json::Value =
                    serde_json::from_str(body["messages"][1]["content"].as_str().unwrap()).unwrap();
                if input["sentences"].as_array().unwrap().len() == 6 {
                    batch_response(request)
                } else {
                    ResponseTemplate::new(200).set_body_json(
                        serde_json::json!({"message":{"content":"{\"translations\":[]}"}}),
                    )
                }
            })
            .mount(&server)
            .await;
        assert!(service
            .execute(
                doc,
                0,
                vec![],
                None,
                0,
                true,
                settings.clone(),
                false,
                Arc::new(AtomicBool::new(false))
            )
            .await
            .is_err());
        let saved = service.db.subtitle_document("v", &key).unwrap().unwrap();
        assert_eq!(saved.translated_count(), 6);
        server.reset().await;
        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .respond_with(|r: &wiremock::Request| {
                batch_response(r).set_delay(Duration::from_secs(2))
            })
            .mount(&server)
            .await;
        let flag = Arc::new(AtomicBool::new(false));
        let cancel = flag.clone();
        let stop = async {
            tokio::time::timeout(Duration::from_secs(3), async {
                while server.received_requests().await.unwrap().is_empty() {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .unwrap();
            cancel.store(true, Ordering::SeqCst);
        };
        let (result, ()) = tokio::join!(
            service.execute(
                saved,
                0,
                vec![],
                None,
                0,
                true,
                settings.clone(),
                false,
                flag
            ),
            stop
        );
        assert_eq!(result.unwrap_err().message, "已取消");
        let saved = service.db.subtitle_document("v", &key).unwrap().unwrap();
        assert_eq!(saved.translated_count(), 6);
        server.reset().await;
        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .respond_with(batch_response)
            .expect(1)
            .mount(&server)
            .await;
        service
            .execute(
                saved,
                0,
                vec![],
                None,
                0,
                true,
                settings,
                false,
                Arc::new(AtomicBool::new(false)),
            )
            .await
            .unwrap();
        assert_eq!(
            service
                .db
                .subtitle_document("v", &key)
                .unwrap()
                .unwrap()
                .translated_count(),
            8
        );
    }

    #[test]
    fn t1513_automatic_modes_skip_all_external_languages_and_resume_audio_cache() {
        let root = tempfile::tempdir().unwrap();
        let db =
            Database::open_and_migrate(&root.path().join("d"), &root.path().join("c")).unwrap();
        db.with_conn(|c| { c.execute_batch("INSERT INTO works(id,title,created_at,updated_at) VALUES('w','w','now','now'); INSERT INTO tracks(id,work_id,title,sort_key,created_at) VALUES('t','w','t','1','now'); INSERT INTO track_versions(id,track_id,created_at) VALUES('zh','t','now'),('ja','t','now'); INSERT INTO media_sources(id,version_id,source_kind,remote_file_id,format,availability,created_at,updated_at) VALUES('zh','zh','online_provider','zh','mp3','available','now','now'),('ja','ja','online_provider','ja','mp3','available','now','now');")?; Ok(()) }).unwrap();
        let provider = AsmrOneProvider::new(db.clone());
        let recognition =
            RecognitionService::new(db.clone(), root.path().join("d"), "auto-mode-test");
        let playback = PlaybackService::new(db.clone(), provider.clone());
        let service = SubtitleTaskService::new(db.clone(), recognition, playback, provider);
        for (version, text) in [("zh", "已有中文字幕"), ("ja", "字幕があります")] {
            let file = root.path().join(format!("{version}.srt"));
            let content = format!("1\n00:00:00,000 --> 00:00:01,000\n{text}\n");
            std::fs::write(&file, &content).unwrap();
            db.with_conn(|c| { c.execute("INSERT INTO attachments(id,work_id,version_id,attachment_type,title,absolute_path,created_at) VALUES(?1,'w',?1,'subtitle',?1,?2,'now')", rusqlite::params![version, file.to_string_lossy()])?; Ok(()) }).unwrap();
            for mode in [
                AutoRecognitionMode::Off,
                AutoRecognitionMode::Missing,
                AutoRecognitionMode::All,
            ] {
                let settings = RecognitionSettings {
                    auto_recognition: mode,
                    ..Default::default()
                };
                let doc = service.task_document(version, &settings, true).unwrap();
                assert_eq!(
                    service
                        .automatic_recognition_allowed(&doc, &settings)
                        .unwrap(),
                    mode == AutoRecognitionMode::All
                );
                if mode == AutoRecognitionMode::All {
                    assert!(doc.original.is_empty() && !doc.recognition_complete);
                    assert!(doc.source_key.starts_with("sensevoice:"));
                } else {
                    assert_eq!(doc.original[0].text, text);
                    assert!(doc.recognition_complete);
                }
            }
            assert_eq!(std::fs::read_to_string(file).unwrap(), content);
        }
        let mut partial = SubtitleDocument::new(
            "zh".into(),
            db.audio_subtitle_key("zh").unwrap(),
            vec![SubtitleCueDto {
                index: 0,
                start_ms: 0,
                end_ms: 1000,
                text: "おはよう".into(),
            }],
            false,
        );
        partial.completed_chunks.insert(0);
        partial.original_corrections.insert(0, "手工修正".into());
        db.save_subtitle_document(&partial).unwrap();
        let all = RecognitionSettings {
            auto_recognition: AutoRecognitionMode::All,
            ..Default::default()
        };
        assert_eq!(
            service.task_document("zh", &all, true).unwrap().rendered()[0].text,
            "手工修正"
        );
        db.with_conn(|c| {
            c.execute("DELETE FROM attachments", [])?;
            Ok(())
        })
        .unwrap();
        let missing = RecognitionSettings {
            auto_recognition: AutoRecognitionMode::Missing,
            ..Default::default()
        };
        let resumed = service.task_document("zh", &missing, true).unwrap();
        assert!(!resumed.recognition_complete && resumed.completed_chunks.contains(&0));
        assert!(service
            .automatic_recognition_allowed(&resumed, &missing)
            .unwrap());
        assert!(!service
            .automatic_recognition_allowed(&resumed, &RecognitionSettings::default())
            .unwrap());
        assert!(service
            .recognition
            .snapshot()
            .unwrap()
            .recognition_job
            .state
            .is_empty());
    }
    #[test]
    fn t1513_chinese_detection_includes_bilingual_lines_but_not_kana() {
        let cue = |text: &str| {
            vec![SubtitleCueDto {
                index: 0,
                start_ms: 0,
                end_ms: 1000,
                text: text.into(),
            }]
        };
        assert!(has_chinese(&cue("晚上好")));
        assert!(has_chinese(&cue("こんばんは\n晚上好")));
        assert!(has_chinese(&cue("こんばんは // 晚上好")));
        assert!(!has_chinese(&cue("字幕を読みます")));
        assert!(!has_chinese(&cue("Good evening")));
    }
    #[test]
    fn t1513_seek_prioritizes_unfinished_chunks_and_resume_skips_saved_chunks() {
        let mut done = std::collections::BTreeSet::from([3, 4]);
        assert_eq!(next_chunk(6, 3, &done), Some(5));
        done.insert(5);
        assert_eq!(next_chunk(6, 3, &done), Some(0));
        done.extend([0, 1, 2]);
        assert_eq!(next_chunk(6, 0, &done), None);
        assert_eq!(next_chunk(0, 0, &done), None);
    }
}
