use crate::{db::Database, domain::*};
use chrono::{DateTime, Utc};
use parking_lot::Mutex;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

const KEY: &str = "user_backup_policy_v1";
const MAX_BYTES: usize = 64 * 1024 * 1024;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BackupPolicy {
    pub interval_hours: u32,
    pub keep_count: usize,
}
impl Default for BackupPolicy {
    fn default() -> Self {
        Self {
            interval_hours: 0,
            keep_count: 7,
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupFile {
    pub id: String,
    pub created_at: String,
    pub kind: String,
    pub bytes: u64,
    pub sha256: String,
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct State {
    policy: BackupPolicy,
    files: Vec<BackupFile>,
    last_success: Option<String>,
    last_attempt: Option<String>,
    last_error: Option<String>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupStatus {
    pub policy: BackupPolicy,
    pub files: Vec<BackupFile>,
    pub last_success: Option<String>,
    pub last_error: Option<String>,
    pub next_due: Option<String>,
    pub directory: String,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RestoreScope {
    Favorites,
    Positions,
    Bookmarks,
    AbLoops,
    Metadata,
    Presets,
    WorkDsp,
    GlobalDsp,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreCount {
    pub scope: RestoreScope,
    pub count: usize,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestorePreview {
    pub token: Option<String>,
    pub schema_version: u64,
    pub exported_at: String,
    pub compatible: bool,
    pub issue: Option<String>,
    pub counts: Vec<RestoreCount>,
    pub matched: Option<BackupRestoreReportDto>,
    pub safety_backup: Option<BackupFile>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreResult {
    pub report: BackupRestoreReportDto,
    pub safety_backup: BackupFile,
    pub dsp_changed: bool,
    pub runtime_warning: Option<String>,
}
struct Pending {
    bundle: UserBackupBundleDto,
    created: Instant,
}
#[derive(Clone)]
pub struct BackupService {
    db: Database,
    gate: Arc<Mutex<()>>,
    previews: Arc<Mutex<HashMap<String, Pending>>>,
}
fn invalid(message: impl Into<String>) -> AppError {
    AppError::new(ErrorCode::InvalidInput, message)
}
fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
fn read_limited(path: &Path) -> AppResult<Vec<u8>> {
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take((MAX_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_BYTES {
        return Err(invalid("备份文件超过 64 MB，未读取"));
    }
    Ok(bytes)
}

/// Export beside the destination, then replace only after a durable verified write.
pub(crate) fn write_export(path: &Path, bytes: &[u8]) -> AppResult<()> {
    if bytes.len() > MAX_BYTES {
        return Err(invalid("备份文件超过 64 MB，原导出文件保留"));
    }
    let parent = path.parent().ok_or_else(|| invalid("导出路径无效"))?;
    let pending = parent.join(format!(".asmr-backup-{}.part", uuid::Uuid::new_v4()));
    let result = (|| -> AppResult<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&pending)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        if read_limited(&pending)? != bytes {
            return Err(invalid("导出备份校验失败，原文件保留"));
        }
        fs::rename(&pending, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&pending);
    }
    result
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use tempfile::{tempdir, TempDir};
    #[test]
    fn export_replaces_only_after_success_and_keeps_locked_previous_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("backup.json");
        write_export(&path, b"previous").unwrap();
        let handle = super::super::storage_file::LockedFile::open(&path, true).unwrap();
        assert!(write_export(&path, b"new").is_err());
        drop(handle);
        assert_eq!(fs::read(&path).unwrap(), b"previous");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
        write_export(&path, b"new").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"new");
    }
    fn fixture() -> (TempDir, BackupService) {
        let dir = tempdir().unwrap();
        let db = Database::open_and_migrate(&dir.path().join("data"), &dir.path().join("cache"))
            .unwrap();
        db.with_conn(|c|{c.execute_batch("INSERT INTO works(id,title,rj_code,created_at,updated_at) VALUES('w','原作品','RJ001','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z'); INSERT INTO tracks(id,work_id,title,sort_key,created_at) VALUES('t','w','音轨','1','now'); INSERT INTO track_versions(id,track_id,created_at) VALUES('v','t','now');")?;Ok(())}).unwrap();
        (dir, BackupService::new(db))
    }
    fn bundle(s: &BackupService) -> UserBackupBundleDto {
        s.db.export_user_backup().unwrap()
    }
    fn preview(s: &BackupService, b: &UserBackupBundleDto) -> RestorePreview {
        s.preview(Some(serde_json::to_string(b).unwrap()), None, None)
            .unwrap()
    }
    #[test]
    fn policy_persists_and_due_checks_cover_restart_wakeup_and_retry() {
        let (_dir, s) = fixture();
        assert_eq!(s.status().unwrap().policy.interval_hours, 0);
        assert!(!s.tick(Utc::now()).unwrap());
        assert!(s
            .set_policy(BackupPolicy {
                interval_hours: 1,
                keep_count: 0
            })
            .is_err());
        s.set_policy(BackupPolicy {
            interval_hours: 24,
            keep_count: 2,
        })
        .unwrap();
        assert!(s.tick(Utc::now()).unwrap());
        assert!(!s.tick(Utc::now()).unwrap());
        let restarted = BackupService::new(s.db.clone());
        assert_eq!(restarted.status().unwrap().policy.keep_count, 2);
        assert!(restarted
            .tick(Utc::now() + chrono::Duration::days(7))
            .unwrap());
        assert_eq!(restarted.status().unwrap().files.len(), 2);
    }
    #[test]
    fn successful_retention_only_prunes_owned_automatic_files() {
        let (_dir, s) = fixture();
        s.set_policy(BackupPolicy {
            interval_hours: 24,
            keep_count: 1,
        })
        .unwrap();
        let manual = s.create(false).unwrap();
        let first = s.create(true).unwrap();
        let second = s.create(true).unwrap();
        assert!(s.path(&manual.id).unwrap().exists());
        assert!(!s.path(&first.id).unwrap().exists());
        assert!(s.path(&second.id).unwrap().exists());
        assert_eq!(s.status().unwrap().files.len(), 2);
        let b: UserBackupBundleDto =
            serde_json::from_slice(&read_limited(&s.path(&second.id).unwrap()).unwrap()).unwrap();
        assert_eq!(b.schema_version, 1);
    }
    #[test]
    fn failed_backup_never_prunes_previous_valid_copy_or_advances_success() {
        let (_dir, s) = fixture();
        s.set_policy(BackupPolicy {
            interval_hours: 24,
            keep_count: 1,
        })
        .unwrap();
        let old = s.create(true).unwrap();
        let before = s.status().unwrap().last_success;
        s.db.with_conn(|c|{c.execute_batch("CREATE TRIGGER fail_backup BEFORE UPDATE ON app_settings WHEN NEW.key='user_backup_policy_v1' BEGIN SELECT RAISE(ABORT,'test disk/DB failure'); END;")?;Ok(())}).unwrap();
        assert!(s.create(true).is_err());
        assert!(s.path(&old.id).unwrap().exists());
        assert_eq!(s.status().unwrap().last_success, before);
    }
    #[test]
    fn future_corrupt_and_unsafe_backups_do_not_write_safety_copies() {
        let (_dir, s) = fixture();
        let mut b = bundle(&s);
        b.schema_version = 999;
        let p = preview(&s, &b);
        assert!(!p.compatible);
        assert!(p.token.is_none());
        b.schema_version = 1;
        b.dsp_global_config.user_preamp_db = 99.;
        assert!(!preview(&s, &b).compatible);
        assert!(s.preview(Some("not JSON".into()), None, None).is_err());
        assert!(s.status().unwrap().files.is_empty());
    }
    #[test]
    fn preview_snapshots_current_data_and_scope_restore_is_single_use() {
        let (_dir, s) = fixture();
        s.db.set_work_override("w", "title", "set", Some("当前标题"))
            .unwrap();
        let mut b = bundle(&s);
        b.work_overrides[0].value_text = Some("备份标题".into());
        b.favorites.push(BackupFavoriteRecordDto {
            work_id: "w".into(),
            rj_code: None,
            work_title: "原作品".into(),
            created_at: Utc::now().to_rfc3339(),
        });
        let p = preview(&s, &b);
        assert!(p.compatible, "{:?}", p.issue);
        assert_eq!(s.db.get_work_detail("w").unwrap().title, "当前标题");
        assert_eq!(s.status().unwrap().files.len(), 1);
        let token = p.token.unwrap();
        let r = s.restore(&token, vec![RestoreScope::Favorites]).unwrap();
        assert_eq!(r.report.favorites_restored, 1);
        assert!(!r.dsp_changed);
        assert_eq!(s.db.get_work_detail("w").unwrap().title, "当前标题");
        assert!(s.restore(&token, vec![RestoreScope::Metadata]).is_err());
        assert_eq!(s.status().unwrap().files.len(), 2);
    }
    #[test]
    fn failed_pre_restore_snapshot_blocks_mutation_and_keeps_old_backup() {
        let (_dir, s) = fixture();
        let old = s.create(false).unwrap();
        let mut b = bundle(&s);
        b.work_overrides.push(BackupWorkOverrideRecordDto {
            work_id: "w".into(),
            rj_code: None,
            field: "title".into(),
            override_mode: "set".into(),
            value_text: Some("新标题".into()),
            updated_at: Utc::now().to_rfc3339(),
        });
        let p = preview(&s, &b);
        assert!(p.compatible);
        // Make the managed directory unavailable without deleting any backup.
        let moved = s.db.data_dir.join("preserved-backups");
        fs::rename(s.directory(), &moved).unwrap();
        fs::write(s.directory(), b"blocked").unwrap();
        assert!(s
            .restore(&p.token.unwrap(), vec![RestoreScope::Metadata])
            .is_err());
        assert_eq!(s.db.get_work_detail("w").unwrap().title, "原作品");
        assert!(moved.join(format!("{}.json", old.id)).exists());
    }
    #[test]
    fn fresh_safety_snapshot_captures_edits_since_preview_and_restore_is_atomic() {
        let (_dir, s) = fixture();
        let mut b = bundle(&s);
        b.work_overrides.push(BackupWorkOverrideRecordDto {
            work_id: "w".into(),
            rj_code: None,
            field: "title".into(),
            override_mode: "set".into(),
            value_text: Some("恢复标题".into()),
            updated_at: Utc::now().to_rfc3339(),
        });
        let p = preview(&s, &b);
        s.db.set_work_override("w", "title", "set", Some("预览后编辑"))
            .unwrap();
        let r = s
            .restore(&p.token.unwrap(), vec![RestoreScope::Metadata])
            .unwrap();
        let snapshot: UserBackupBundleDto =
            serde_json::from_slice(&read_limited(&s.path(&r.safety_backup.id).unwrap()).unwrap())
                .unwrap();
        assert_eq!(
            snapshot.work_overrides[0].value_text.as_deref(),
            Some("预览后编辑")
        );
        assert_eq!(s.db.get_work_detail("w").unwrap().title, "恢复标题");
        let p = preview(&s, &b);
        s.db.with_conn(|c|{c.execute_batch("CREATE TRIGGER deny_override BEFORE UPDATE ON work_overrides BEGIN SELECT RAISE(ABORT,'restore denied'); END;")?;Ok(())}).unwrap();
        assert!(s
            .restore(&p.token.unwrap(), vec![RestoreScope::Metadata])
            .is_err());
        assert_eq!(s.db.get_work_detail("w").unwrap().title, "恢复标题");
    }
    #[test]
    fn missing_version_never_uses_title_fallback_and_encrypted_preview_is_supported() {
        let (_dir, s) = fixture();
        let mut b = bundle(&s);
        b.playback_positions.push(BackupPositionRecordDto {
            version_id: "other-edit".into(),
            rj_code: Some("RJ001".into()),
            track_title: "音轨".into(),
            version_label: "other".into(),
            position_ms: 9000,
            completed: false,
            updated_at: Utc::now().to_rfc3339(),
        });
        let env = encrypt_backup_bundle(&b, "test-passphrase").unwrap();
        let raw = serde_json::to_string(&env).unwrap();
        assert!(s
            .preview(Some(raw.clone()), None, Some("wrong".into()))
            .is_err());
        let p = s
            .preview(Some(raw), None, Some("test-passphrase".into()))
            .unwrap();
        assert!(p.compatible, "{:?}", p.issue);
        assert_eq!(p.matched.unwrap().positions_restored, 0);
        assert_eq!(
            s.restore(&p.token.unwrap(), vec![RestoreScope::Positions])
                .unwrap()
                .report
                .positions_restored,
            0
        );
    }
    #[test]
    fn corrupt_managed_file_and_expired_preview_are_rejected() {
        let (_dir, s) = fixture();
        let file = s.create(false).unwrap();
        fs::write(s.path(&file.id).unwrap(), b"{}").unwrap();
        assert!(s.preview(None, Some(file.id), None).is_err());
        let p = preview(&s, &bundle(&s));
        let token = p.token.unwrap();
        s.previews.lock().get_mut(&token).unwrap().created =
            Instant::now() - Duration::from_secs(601);
        assert!(s.restore(&token, vec![RestoreScope::Favorites]).is_err());
    }
}
fn validate_policy(policy: &BackupPolicy) -> AppResult<()> {
    if ![0, 24, 72, 168].contains(&policy.interval_hours) || !(1..=30).contains(&policy.keep_count)
    {
        return Err(invalid("备份周期或保留份数无效"));
    }
    Ok(())
}
fn load(conn: &rusqlite::Connection) -> AppResult<State> {
    let raw: Option<String> = conn
        .query_row(
            "SELECT value_json FROM app_settings WHERE key=?1",
            [KEY],
            |r| r.get(0),
        )
        .optional()?;
    let state: State = raw
        .map(|raw| {
            serde_json::from_str(&raw)
                .map_err(|_| invalid("备份策略记录损坏，请保留文件并检查诊断"))
        })
        .transpose()?
        .unwrap_or_default();
    validate_policy(&state.policy)?;
    Ok(state)
}
fn save(conn: &rusqlite::Connection, state: &State) -> AppResult<()> {
    let raw = serde_json::to_string(state).map_err(|_| invalid("无法编码备份策略"))?;
    conn.execute("INSERT INTO app_settings(key,value_json,updated_at) VALUES(?1,?2,?3) ON CONFLICT(key) DO UPDATE SET value_json=excluded.value_json,updated_at=excluded.updated_at",params![KEY,raw,Utc::now().to_rfc3339()])?;
    Ok(())
}
fn validate(bundle: &UserBackupBundleDto) -> AppResult<()> {
    if bundle.schema_version != 1 {
        return Err(invalid("不支持此备份格式版本，请使用匹配版本的播放器"));
    }
    if DateTime::parse_from_rfc3339(&bundle.exported_at).is_err() {
        return Err(invalid("备份时间无效"));
    }
    if counts(bundle).iter().any(|c| c.count > 100_000) {
        return Err(invalid("备份记录数量超过限制"));
    }
    if bundle
        .playback_positions
        .iter()
        .any(|p| p.position_ms < 0 || DateTime::parse_from_rfc3339(&p.updated_at).is_err())
        || bundle
            .ab_loops
            .iter()
            .any(|p| p.start_ms < 0 || p.start_ms >= p.end_ms)
    {
        return Err(invalid("备份包含无效的时间轴记录"));
    }
    if bundle
        .work_overrides
        .iter()
        .any(|v| !["set", "cleared"].contains(&v.override_mode.as_str()))
    {
        return Err(invalid("元数据覆盖模式无效"));
    }
    let builtins: HashSet<_> = builtin_dsp_presets().into_iter().map(|p| p.id).collect();
    if bundle
        .custom_dsp_presets
        .iter()
        .any(|p| p.is_builtin || builtins.contains(&p.id) || p.name.trim().is_empty())
    {
        return Err(invalid("备份不能覆盖内置音效预设"));
    }
    crate::db::bookmark_notes::validate_records(&bundle.bookmarks)?;
    for config in std::iter::once(&bundle.dsp_global_config)
        .chain(bundle.custom_dsp_presets.iter().map(|p| &p.config))
        .chain(bundle.work_dsp_overrides.iter().map(|p| &p.config))
    {
        let mut cfg = config.clone();
        validate_dsp_config(&mut cfg)?;
        if serde_json::to_value(&cfg).ok() != serde_json::to_value(config).ok() {
            return Err(invalid("备份音效参数需要修正，未恢复"));
        }
    }
    Ok(())
}
fn counts(b: &UserBackupBundleDto) -> Vec<RestoreCount> {
    use RestoreScope::*;
    [
        (Favorites, b.favorites.len()),
        (Positions, b.playback_positions.len()),
        (Bookmarks, b.bookmarks.len()),
        (AbLoops, b.ab_loops.len()),
        (Metadata, b.work_overrides.len()),
        (Presets, b.custom_dsp_presets.len()),
        (WorkDsp, b.work_dsp_overrides.len()),
        (GlobalDsp, 1),
    ]
    .into_iter()
    .map(|(scope, count)| RestoreCount { scope, count })
    .collect()
}
fn select(mut b: UserBackupBundleDto, scopes: &[RestoreScope]) -> UserBackupBundleDto {
    use RestoreScope::*;
    if !scopes.contains(&Favorites) {
        b.favorites.clear();
    }
    if !scopes.contains(&Positions) {
        b.playback_positions.clear();
    }
    if !scopes.contains(&Bookmarks) {
        b.bookmarks.clear();
    }
    if !scopes.contains(&AbLoops) {
        b.ab_loops.clear();
    }
    if !scopes.contains(&Metadata) {
        b.work_overrides.clear();
    }
    if !scopes.contains(&Presets) {
        b.custom_dsp_presets.clear();
    }
    if !scopes.contains(&WorkDsp) {
        b.work_dsp_overrides.clear();
    }
    b
}
impl BackupService {
    pub fn new(db: Database) -> Self {
        Self {
            db,
            gate: Arc::new(Mutex::new(())),
            previews: Arc::new(Mutex::new(HashMap::new())),
        }
    }
    fn directory(&self) -> PathBuf {
        self.db.data_dir.join("user-backups-v1")
    }
    fn path(&self, id: &str) -> AppResult<PathBuf> {
        uuid::Uuid::parse_str(id).map_err(|_| invalid("备份 ID 无效"))?;
        Ok(self.directory().join(format!("{id}.json")))
    }
    fn ensure_directory(&self) -> AppResult<()> {
        // Managed backup roots never follow directory junctions, including ancestors.
        let dir = self.directory();
        for p in dir.ancestors() {
            if let Ok(m) = fs::symlink_metadata(p) {
                #[cfg(windows)]
                {
                    use std::os::windows::fs::MetadataExt;
                    if m.file_attributes() & 0x400 != 0 {
                        return Err(invalid("备份目录不能使用链接或目录联接"));
                    }
                }
                if m.file_type().is_symlink() || !m.is_dir() {
                    return Err(invalid("备份目录不可用"));
                }
            }
        }
        fs::create_dir_all(dir)?;
        Ok(())
    }
    fn write_bundle(&self, b: &UserBackupBundleDto, kind: &str) -> AppResult<BackupFile> {
        validate(b)?;
        self.ensure_directory()?;
        let id = uuid::Uuid::new_v4().to_string();
        let path = self.path(&id)?;
        let pending = path.with_extension("json.part");
        let bytes = serde_json::to_vec(b).map_err(|_| invalid("无法编码备份"))?;
        if bytes.len() > MAX_BYTES {
            return Err(invalid("用户资产超过 64 MB，备份未写入，旧副本保留"));
        }
        let result = (|| -> AppResult<()> {
            let mut f = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&pending)?;
            f.write_all(&bytes)?;
            f.sync_all()?;
            drop(f);
            let read = read_limited(&pending)?;
            if read != bytes {
                return Err(invalid("备份写入校验失败，旧副本保留"));
            }
            let _: UserBackupBundleDto =
                serde_json::from_slice(&read).map_err(|_| invalid("备份结构校验失败"))?;
            fs::rename(&pending, &path)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&pending);
        }
        result?;
        Ok(BackupFile {
            id,
            created_at: Utc::now().to_rfc3339(),
            kind: kind.into(),
            bytes: bytes.len() as u64,
            sha256: digest(&bytes),
        })
    }
    fn snapshot(
        &self,
        conn: &rusqlite::Connection,
        state: &mut State,
        kind: &str,
    ) -> AppResult<BackupFile> {
        let b = Database::export_user_backup_on_conn(conn)?;
        let file = self.write_bundle(&b, kind)?;
        state.files.push(file.clone());
        save(conn, state)?;
        Ok(file)
    }
    pub fn status(&self) -> AppResult<BackupStatus> {
        let state = self.db.with_conn(|c| load(c))?;
        let next_due = if state.policy.interval_hours == 0 {
            None
        } else {
            state
                .last_success
                .as_ref()
                .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
                .map(|t| {
                    (t + chrono::Duration::hours(state.policy.interval_hours as i64)).to_rfc3339()
                })
        };
        Ok(BackupStatus {
            policy: state.policy,
            files: state.files.into_iter().rev().collect(),
            last_success: state.last_success,
            last_error: state.last_error,
            next_due,
            directory: self.directory().to_string_lossy().into(),
        })
    }
    pub fn set_policy(&self, policy: BackupPolicy) -> AppResult<BackupStatus> {
        validate_policy(&policy)?;
        let _gate = self.gate.lock();
        self.db.with_conn(|c| {
            let mut s = load(c)?;
            s.policy = policy;
            save(c, &s)
        })?;
        self.status()
    }
    pub fn create(&self, automatic: bool) -> AppResult<BackupFile> {
        let _gate = self.gate.lock();
        self.create_locked(automatic)
    }
    fn create_locked(&self, automatic: bool) -> AppResult<BackupFile> {
        let result = self.db.with_conn(|conn| {
            let mut state = load(conn)?;
            state.last_attempt = Some(Utc::now().to_rfc3339());
            save(conn, &state)?;
            let file = self.snapshot(
                conn,
                &mut state,
                if automatic { "automatic" } else { "manual" },
            )?;
            if automatic {
                state.last_success = Some(file.created_at.clone());
            }
            state.last_error = None;
            save(conn, &state)?;
            Ok(file)
        });
        if let Err(err) = &result {
            let _ = self.db.with_conn(|c| {
                let mut s = load(c)?;
                s.last_error = Some(err.message.clone());
                save(c, &s)
            });
        }
        if result.is_ok() && automatic {
            self.prune()?;
        }
        result
    }
    fn prune(&self) -> AppResult<()> {
        self.ensure_directory()?;
        let mut state = self.db.with_conn(|c| load(c))?;
        let automatic: Vec<_> = state
            .files
            .iter()
            .filter(|f| f.kind == "automatic")
            .cloned()
            .collect();
        for f in automatic
            .iter()
            .take(automatic.len().saturating_sub(state.policy.keep_count))
        {
            let result = (|| -> AppResult<()> {
                let path = self.path(&f.id)?;
                let probe = super::storage_file::LockedFile::open(&path, false)?;
                let id = probe.identity()?;
                drop(probe);
                if digest(&read_limited(&path)?) != f.sha256 {
                    return Err(invalid("旧备份校验失败，已保留"));
                }
                let handle = super::storage_file::LockedFile::open(&path, true)?;
                if handle.identity()? != id {
                    return Err(invalid("旧备份发生变化，已保留"));
                }
                handle.delete()?;
                Ok(())
            })();
            match result {
                Ok(()) => state.files.retain(|v| v.id != f.id),
                Err(e) => {
                    state.last_error = Some(format!(
                        "新备份已保存；清理旧副本失败，文件保留：{}",
                        e.message
                    ))
                }
            }
        }
        self.db.with_conn(|c| save(c, &state))
    }
    pub fn tick(&self, now: DateTime<Utc>) -> AppResult<bool> {
        let Some(_gate) = self.gate.try_lock() else {
            return Ok(false);
        };
        let s = self.db.with_conn(|c| load(c))?;
        if s.policy.interval_hours == 0 {
            return Ok(false);
        }
        let due = s
            .last_success
            .as_ref()
            .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
            .is_none_or(|t| {
                now.signed_duration_since(t).num_hours() >= s.policy.interval_hours as i64
            });
        let retry = s
            .last_attempt
            .as_ref()
            .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
            .is_none_or(|t| now.signed_duration_since(t).num_seconds() >= 300);
        if due && retry {
            self.create_locked(true)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }
    pub fn preview(
        &self,
        raw: Option<String>,
        file_id: Option<String>,
        passphrase: Option<String>,
    ) -> AppResult<RestorePreview> {
        let _gate = self.gate.lock();
        let bytes = if let Some(id) = file_id {
            self.ensure_directory()?;
            let state = self.db.with_conn(|c| load(c))?;
            let file = state
                .files
                .iter()
                .find(|f| f.id == id)
                .ok_or_else(|| invalid("备份记录不存在"))?;
            let bytes = read_limited(&self.path(&id)?)?;
            if digest(&bytes) != file.sha256 {
                return Err(invalid("备份校验失败，请保留文件并使用其他副本"));
            }
            bytes
        } else {
            raw.ok_or_else(|| invalid("请选择备份文件"))?.into_bytes()
        };
        if bytes.len() > MAX_BYTES {
            return Err(invalid("备份文件超过 64 MB"));
        }
        let json: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|_| invalid("无法读取备份 JSON"))?;
        let json = if let Some(pass) = passphrase {
            let env: EncryptedSyncEnvelopeDto =
                serde_json::from_value(json).map_err(|_| invalid("加密同步包格式无效"))?;
            if env.version != 1 || env.kdf_rounds == 0 || env.kdf_rounds > 65536 {
                return Err(invalid("不支持此加密同步包版本或参数"));
            }
            serde_json::to_value(decrypt_backup_bundle(&env, &pass)?)
                .map_err(|_| invalid("无法读取解密后的备份"))?
        } else {
            json
        };
        let version = json
            .get("schemaVersion")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let exported = json
            .get("exportedAt")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_owned();
        let mut preview = RestorePreview {
            token: None,
            schema_version: version,
            exported_at: exported,
            compatible: false,
            issue: None,
            counts: vec![],
            matched: None,
            safety_backup: None,
        };
        if version != 1 {
            preview.issue = Some("不支持此备份格式版本；未修改当前数据".into());
            return Ok(preview);
        }
        let bundle: UserBackupBundleDto = match serde_json::from_value(json) {
            Ok(v) => v,
            Err(_) => {
                preview.issue = Some("备份缺少必要字段或字段类型无效".into());
                return Ok(preview);
            }
        };
        preview.counts = counts(&bundle);
        if let Err(e) = validate(&bundle) {
            preview.issue = Some(e.message);
            return Ok(preview);
        }
        let prepared = self.db.with_conn(|conn| {
            let matched = Database::restore_user_backup_on_conn(conn, &bundle, false, true, true)?;
            let mut s = load(conn)?;
            let backup = self.snapshot(conn, &mut s, "before_restore")?;
            Ok((matched, backup))
        });
        let (matched, backup) = match prepared {
            Ok(v) => v,
            Err(e) => {
                preview.issue = Some(format!("无法准备恢复：{}。当前数据未修改。", e.message));
                return Ok(preview);
            }
        };
        preview.compatible = true;
        preview.matched = Some(matched);
        preview.safety_backup = Some(backup);
        let token = uuid::Uuid::new_v4().to_string();
        let mut pending = self.previews.lock();
        pending.retain(|_, p| p.created.elapsed() < Duration::from_secs(600));
        if pending.len() >= 4 {
            pending.clear()
        }
        pending.insert(
            token.clone(),
            Pending {
                bundle,
                created: Instant::now(),
            },
        );
        preview.token = Some(token);
        Ok(preview)
    }
    pub fn restore(&self, token: &str, scopes: Vec<RestoreScope>) -> AppResult<RestoreResult> {
        if scopes.is_empty() {
            return Err(invalid("请至少选择一个恢复范围"));
        }
        let _gate = self.gate.lock();
        let pending = self
            .previews
            .lock()
            .remove(token)
            .ok_or_else(|| invalid("恢复预览已失效，请重新预览"))?;
        if pending.created.elapsed() >= Duration::from_secs(600) {
            return Err(invalid("恢复预览已过期，请重新预览"));
        }
        let bundle = select(pending.bundle, &scopes);
        let dsp = scopes.contains(&RestoreScope::GlobalDsp);
        self.db.with_conn(|conn| {
            // Revalidate on the same locked connection, then snapshot immediately before commit.
            Database::restore_user_backup_on_conn(conn, &bundle, false, dsp, true)?;
            let mut s = load(conn)?;
            let backup = self.snapshot(conn, &mut s, "before_restore")?;
            let report = Database::restore_user_backup_on_conn(conn, &bundle, true, dsp, true)?;
            Ok(RestoreResult {
                report,
                safety_backup: backup,
                dsp_changed: dsp
                    || scopes.contains(&RestoreScope::WorkDsp)
                    || scopes.contains(&RestoreScope::Presets),
                runtime_warning: None,
            })
        })
    }
}
