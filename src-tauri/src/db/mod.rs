pub mod repo;
pub mod smart_collections;
pub mod bulk_library;
pub mod bookmark_notes;
pub mod queue_plans;
pub mod shortcuts;
pub mod listening;
pub mod affinity;

use crate::domain::{redact_sensitive, AppError, AppResult, ErrorCode};
use chrono::Utc;
use parking_lot::Mutex;
use rusqlite::{params, Connection};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

const MIGRATIONS: &[(i64, &str, &str)] = &[
    // Append migrations below in ascending order to preserve upgrade backups.
    (
        1,
        "001_initial_core",
        include_str!("../../migrations/001_initial_core.sql"),
    ),
    (
        2,
        "002_favorites_and_links",
        include_str!("../../migrations/002_favorites_and_links.sql"),
    ),
    (
        3,
        "003_downloads",
        include_str!("../../migrations/003_downloads.sql"),
    ),
    (
        4,
        "004_asmr_experience",
        include_str!("../../migrations/004_asmr_experience.sql"),
    ),
    (
        5,
        "005_pro_audio_dsp",
        include_str!("../../migrations/005_pro_audio_dsp.sql"),
    ),
    (
        6,
        "006_performance_fts_and_indexes",
        include_str!("../../migrations/006_performance_fts_and_indexes.sql"),
    ),
    (
        7,
        "007_collected_works",
        include_str!("../../migrations/007_collected_works.sql"),
    ),
    (
        8,
        "008_volume_max_boost",
        include_str!("../../migrations/008_volume_max_boost.sql"),
    ),
    (9, "009_smart_collections", include_str!("../../migrations/009_smart_collections.sql")),
    (10, "010_bulk_library", include_str!("../../migrations/010_bulk_library.sql")),
    (11, "011_bookmark_notes", include_str!("../../migrations/011_bookmark_notes.sql")),
    (12, "012_queue_plans", include_str!("../../migrations/012_queue_plans.sql")),
    (13, "013_download_schedule", include_str!("../../migrations/013_download_schedule.sql")),
    (14, "014_listening_completion", include_str!("../../migrations/014_listening_completion.sql")),
    (15, "015_favorite_versions", include_str!("../../migrations/015_favorite_versions.sql")),
];

#[derive(Clone)]
pub struct Database {
    conn: Arc<Mutex<Connection>>,
    pub data_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub log_dir: PathBuf,
    pub db_path: PathBuf,
}

impl Database {
    fn check_connection_integrity(conn: &Connection) -> bool {
        match conn.query_row("PRAGMA quick_check;", [], |r| r.get::<_, String>(0)) {
            Ok(res) => res.eq_ignore_ascii_case("ok"),
            Err(_) => false,
        }
    }

    fn try_restore_latest_backup(data_dir: &Path, db_path: &Path) -> Option<PathBuf> {
        let mut backups: Vec<PathBuf> = fs::read_dir(data_dir)
            .ok()?
            .filter_map(|e| e.ok().map(|entry| entry.path()))
            .filter(|p| {
                p.is_file()
                    && p.file_name()
                        .and_then(|n| n.to_str())
                        .map(|n| n.starts_with("asmr_player.backup-") && n.ends_with(".db"))
                        .unwrap_or(false)
            })
            .collect();
        backups.sort();
        let latest = backups.pop()?;

        let corrupted_archive = data_dir.join(format!(
            "asmr_player.corrupted-{}.db",
            Utc::now().format("%Y%m%d%H%M%S")
        ));
        let _ = fs::rename(db_path, &corrupted_archive);
        let _ = fs::remove_file(data_dir.join("asmr_player.db-wal"));
        let _ = fs::remove_file(data_dir.join("asmr_player.db-shm"));

        if fs::copy(&latest, db_path).is_ok() {
            Some(latest)
        } else {
            None
        }
    }

    pub fn open_and_migrate(data_dir: &Path, cache_dir: &Path) -> AppResult<Self> {
        fs::create_dir_all(data_dir)?;
        fs::create_dir_all(cache_dir)?;
        let log_dir = data_dir.join("logs");
        fs::create_dir_all(&log_dir)?;

        let db_path = data_dir.join("asmr_player.db");
        let db_existed = db_path.exists();
        let mut restored_from_backup: Option<PathBuf> = None;

        let mut conn = match Connection::open(&db_path) {
            Ok(c) if !db_existed || Self::check_connection_integrity(&c) => c,
            _ => {
                if let Some(backup_file) = Self::try_restore_latest_backup(data_dir, &db_path) {
                    restored_from_backup = Some(backup_file);
                    Connection::open(&db_path).map_err(|e| {
                        AppError::new(
                            ErrorCode::DatabaseError,
                            format!("从备份恢复数据库后仍无法打开 {}: {}", db_path.display(), e),
                        )
                    })?
                } else {
                    return Err(AppError::new(
                        ErrorCode::DatabaseError,
                        format!("数据库完整性校验 (PRAGMA quick_check) 失败且无可用备份: {}", db_path.display()),
                    ));
                }
            }
        };

        conn.execute_batch(
            "PRAGMA foreign_keys = ON;
             PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA busy_timeout = 5000;",
        )?;

        Self::run_migrations(&mut conn, &db_path, db_existed)?;

        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
            data_dir: data_dir.to_path_buf(),
            cache_dir: cache_dir.to_path_buf(),
            log_dir,
            db_path,
        };
        if let Some(restored) = restored_from_backup {
            db.append_log(
                "WARN",
                &format!(
                    "Detected corrupted SQLite database on startup; automatically restored from backup {}",
                    restored.display()
                ),
            );
        }
        db.append_log("INFO", "Database initialized and migrations verified.");
        Ok(db)
    }

    /// T1003: Checkpoints and truncates the SQLite WAL file (`-wal`) on clean shutdown or cache maintenance.
    pub fn wal_checkpoint_truncate(&self) -> AppResult<()> {
        self.with_conn(|conn| {
            conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
            Ok(())
        })
    }

    pub fn run_migrations(
        conn: &mut Connection,
        db_path: &Path,
        db_existed: bool,
    ) -> AppResult<usize> {
        conn.execute(
            "CREATE TABLE IF NOT EXISTS _schema_migrations (
                version INTEGER PRIMARY KEY NOT NULL,
                name TEXT NOT NULL,
                applied_at TEXT NOT NULL
            )",
            [],
        )
        .map_err(|e| {
            AppError::new(
                ErrorCode::MigrationError,
                format!("无法初始化迁移记录表: {}", e),
            )
        })?;

        let current_version: i64 = conn
            .query_row(
                "SELECT COALESCE(MAX(version), 0) FROM _schema_migrations",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);

        let pending: Vec<&(i64, &str, &str)> = MIGRATIONS
            .iter()
            .filter(|(v, _, _)| *v > current_version)
            .collect();

        if pending.is_empty() {
            return Ok(0);
        }

        // T704: Backup existing database before applying schema upgrades
        let mut backup_hint = None;
        if db_existed && current_version > 0 {
            let backup_path = db_path.with_extension(format!(
                "backup-v{}-{}.db",
                current_version,
                Utc::now().format("%Y%m%d%H%M%S")
            ));
            // T1405: SQLite's backup API includes committed WAL pages. Copying only
            // the main file can silently omit the latest library and listening records.
            conn.backup(rusqlite::DatabaseName::Main, &backup_path, None).map_err(|e| {
                AppError::new(ErrorCode::MigrationError, format!("升级前数据库备份失败，已停止迁移: {e}"))
            })?;
            backup_hint = Some(format!(
                    "已自动生成升级前备份：{}，可将该备份重命名恢复。",
                    backup_path.display()
                ));
        }

        let mut applied_count = 0;
        for (version, name, sql) in pending {
            let tx = conn.transaction().map_err(|e| {
                AppError::new(
                    ErrorCode::MigrationError,
                    format!("开启迁移事务失败 ({}): {}", name, e),
                )
            })?;

            if let Err(err) = tx.execute_batch(sql) {
                let mut app_err = AppError::new(
                    ErrorCode::MigrationError,
                    format!("迁移 {} (v{}) 执行失败: {}", name, version, err),
                );
                if let Some(ref hint) = backup_hint {
                    app_err = app_err.with_hint(hint.clone());
                }
                return Err(app_err);
            }

            tx.execute(
                "INSERT INTO _schema_migrations (version, name, applied_at) VALUES (?1, ?2, ?3)",
                params![version, name, Utc::now().to_rfc3339()],
            )
            .map_err(|e| {
                AppError::new(
                    ErrorCode::MigrationError,
                    format!("记录迁移版本失败 ({}): {}", name, e),
                )
            })?;

            tx.commit().map_err(|e| {
                AppError::new(
                    ErrorCode::MigrationError,
                    format!("提交迁移事务失败 ({}): {}", name, e),
                )
            })?;
            applied_count += 1;
        }

        Ok(applied_count)
    }

    pub fn with_conn<F, T>(&self, f: F) -> AppResult<T>
    where
        F: FnOnce(&mut Connection) -> AppResult<T>,
    {
        let mut guard = self.conn.lock();
        f(&mut guard)
    }

    /// Appends a sanitized log entry with simple 2 MB file rotation (T107).
    pub fn append_log(&self, level: &str, message: &str) {
        let clean = redact_sensitive(message);
        let log_file = self.log_dir.join("app.log");
        if let Ok(meta) = fs::metadata(&log_file) {
            if meta.len() > 2 * 1024 * 1024 {
                let rotated = self.log_dir.join("app.log.1");
                let _ = fs::rename(&log_file, rotated);
            }
        }
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&log_file) {
            let _ = writeln!(
                file,
                "[{}] [{}] {}",
                Utc::now().to_rfc3339(),
                level,
                clean
            );
        }
    }

    pub fn read_recent_logs(&self, max_lines: usize) -> Vec<String> {
        let log_file = self.log_dir.join("app.log");
        if let Ok(content) = fs::read_to_string(log_file) {
            let lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
            let start = lines.len().saturating_sub(max_lines);
            lines[start..].to_vec()
        } else {
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn upgrade_backup_contains_uncheckpointed_wal_records_t1405() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("asmr_player.db");
        let mut conn = Connection::open(&path).unwrap();
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0;").unwrap();
        // Build the old schema itself, rather than replaying ALTER TABLE on new columns.
        conn.execute_batch("CREATE TABLE _schema_migrations(version INTEGER PRIMARY KEY, name TEXT NOT NULL, applied_at TEXT NOT NULL);").unwrap();
        for (version, name, sql) in MIGRATIONS.iter().filter(|(version, _, _)| *version <= 7) {
            conn.execute_batch(sql).unwrap();
            conn.execute("INSERT INTO _schema_migrations VALUES (?1,?2,'now')", params![version,name]).unwrap();
        }
        conn.execute("INSERT INTO works (id,title,created_at,updated_at) VALUES ('wal','WAL work','now','now')", []).unwrap();
        Database::run_migrations(&mut conn, &path, true).unwrap();
        let backup = fs::read_dir(dir.path()).unwrap().flatten().map(|e| e.path())
            .find(|p| p.file_name().unwrap().to_string_lossy().contains("backup-v7")).unwrap();
        let copy = Connection::open(backup).unwrap();
        let title: String = copy.query_row("SELECT title FROM works WHERE id='wal'", [], |r| r.get(0)).unwrap();
        assert_eq!(title, "WAL work");
    }

    #[test]
    fn bookmark_upgrade_preserves_existing_notes_t1503() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("legacy.db");
        let mut conn = Connection::open(&path).unwrap();
        conn.execute_batch("CREATE TABLE _schema_migrations(version INTEGER PRIMARY KEY, name TEXT NOT NULL, applied_at TEXT NOT NULL);").unwrap();
        for (version, name, sql) in MIGRATIONS.iter().filter(|(version, _, _)| *version <= 10) {
            conn.execute_batch(sql).unwrap();
            conn.execute("INSERT INTO _schema_migrations VALUES (?1,?2,'now')", params![version,name]).unwrap();
        }
        conn.execute_batch("INSERT INTO works(id,title,created_at,updated_at) VALUES('w','Work','now','now');
            INSERT INTO tracks(id,work_id,title,sort_key,created_at) VALUES('t','w','Track','1','now');
            INSERT INTO track_versions(id,track_id,version_label,created_at) VALUES('v','t','Original','now');
            INSERT INTO bookmarks(id,version_id,position_ms,note,created_at,updated_at) VALUES('b','v',12500,'旧书签笔记','now','now');").unwrap();
        Database::run_migrations(&mut conn, &path, true).unwrap();
        let record = bookmark_notes::read_records(&conn).unwrap().pop().unwrap();
        assert_eq!(record.version_id, "v");
        assert_eq!(record.note, "旧书签笔记");
        assert_eq!(record.position_ms, 12500);
        assert!(record.tags.is_empty());
        assert_eq!(Database::run_migrations(&mut conn, &path, true).unwrap(), 0);
    }

    #[test]
    fn test_migrations_are_idempotent_and_preserve_data() {
        let dir = tempdir().unwrap();
        let data_dir = dir.path().join("data");
        let cache_dir = dir.path().join("cache");

        let db = Database::open_and_migrate(&data_dir, &cache_dir).unwrap();
        db.with_conn(|conn| {
            let fk: i64 = conn.pragma_query_value(None, "foreign_keys", |r| r.get(0))?;
            assert_eq!(fk, 1, "foreign_keys must be enabled");
            Ok(())
        })
        .unwrap();

        // Second run must apply 0 migrations and keep existing database intact
        let mut conn2 = Connection::open(data_dir.join("asmr_player.db")).unwrap();
        let applied_second =
            Database::run_migrations(&mut conn2, &data_dir.join("asmr_player.db"), true).unwrap();
        assert_eq!(applied_second, 0);
    }

    #[test]
    fn test_failed_migration_does_not_destroy_existing_data() {
        let dir = tempdir().unwrap();
        let data_dir = dir.path().join("data");
        let cache_dir = dir.path().join("cache");
        let db = Database::open_and_migrate(&data_dir, &cache_dir).unwrap();

        db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO works (id, title, created_at, updated_at) VALUES ('w1', 'Existing Work', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
                [],
            )?;
            let tx = conn.transaction()?;
            let bad_res = tx.execute_batch("ALTER TABLE non_existent_table ADD COLUMN foo TEXT;");
            assert!(bad_res.is_err());
            drop(tx);
            let count: i64 = conn.query_row("SELECT COUNT(*) FROM works WHERE id = 'w1'", [], |r| r.get(0))?;
            assert_eq!(count, 1, "Existing work must remain intact after failed migration");
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn test_corrupted_db_auto_restores_from_latest_backup_t1003() {
        let dir = tempdir().unwrap();
        let data_dir = dir.path().join("data");
        let cache_dir = dir.path().join("cache");

        {
            let db = Database::open_and_migrate(&data_dir, &cache_dir).unwrap();
            db.with_conn(|conn| {
                conn.execute(
                    "INSERT INTO works (id, title, created_at, updated_at) VALUES ('w_safe', 'Safe Backup Work', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
                    [],
                )?;
                Ok(())
            })
            .unwrap();
            db.wal_checkpoint_truncate().unwrap();
        }

        // Create a valid backup snapshot before corrupting the primary db file
        let db_path = data_dir.join("asmr_player.db");
        let backup_path = data_dir.join("asmr_player.backup-v6-20261012000000.db");
        fs::copy(&db_path, &backup_path).unwrap();

        // Corrupt the primary database file with invalid header bytes
        fs::write(&db_path, b"NOT_A_VALID_SQLITE_DATABASE_HEADER_CORRUPTED_BYTES").unwrap();

        // Re-opening must detect corruption via PRAGMA quick_check and automatically restore from backup!
        let recovered_db = Database::open_and_migrate(&data_dir, &cache_dir).unwrap();
        let detail = recovered_db.get_work_detail("w_safe").unwrap();
        assert_eq!(detail.title, "Safe Backup Work");
    }
}
