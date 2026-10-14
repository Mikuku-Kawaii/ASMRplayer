use crate::domain::{AppResult, ListeningStatus};
use rusqlite::Connection;
use std::collections::HashMap;

pub(super) fn statuses(conn: &Connection) -> AppResult<HashMap<String, ListeningStatus>> {
    let mut result = HashMap::new();
    let mut stmt = conn.prepare("SELECT work_id FROM work_playback_state UNION SELECT t.work_id FROM tracks t JOIN track_versions v ON v.track_id=t.id JOIN playback_positions p ON p.version_id=v.id WHERE p.position_ms>0 OR p.completed=1 OR p.ever_completed=1")?;
    for row in stmt.query_map([], |r| r.get::<_, String>(0))? {
        result.insert(row?, ListeningStatus::InProgress);
    }
    // A complete directory is one listening edition; do not require duplicate
    // WAV/MP3 or alternate edits to be played as well.
    let mut stmt = conn.prepare("WITH track_completion AS (
        SELECT t.work_id,t.logical_dir_path,t.id,MAX(COALESCE(p.ever_completed,0)) AS heard
        FROM tracks t LEFT JOIN track_versions v ON v.track_id=t.id
        LEFT JOIN playback_positions p ON p.version_id=v.id GROUP BY t.id
    ) SELECT DISTINCT work_id FROM track_completion GROUP BY work_id,logical_dir_path HAVING MIN(heard)=1")?;
    for row in stmt.query_map([], |r| r.get::<_, String>(0))? {
        result.insert(row?, ListeningStatus::Played);
    }
    let mut stmt = conn.prepare("SELECT work_id,listened FROM work_listening_marks")?;
    for row in stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, bool>(1)?)))? {
        let (id, listened) = row?;
        result.insert(id, if listened { ListeningStatus::Played } else { ListeningStatus::Unplayed });
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use crate::db::smart_collections::{CollectionFilters, ListeningFilter};
    use rusqlite::params;
    use tempfile::tempdir;

    fn seed(conn: &Connection) {
        conn.execute_batch("INSERT INTO works(id,title,created_at,updated_at) VALUES('w','Work','now','now');
            INSERT INTO collected_works(work_id,created_at) VALUES('w','now');
            INSERT INTO tracks(id,work_id,title,logical_dir_path,sort_key,created_at) VALUES
                ('t1','w','First','WAV','1','now'),('t2','w','Second','WAV','2','now'),('t3','w','MP3 edition','MP3','1','now');
            INSERT INTO track_versions(id,track_id,version_label,created_at) VALUES('v1','t1','Original','now'),('v2','t2','Original','now'),('v3','t3','Original','now');").unwrap();
    }
    fn status(db: &Database) -> ListeningStatus {
        db.list_works("all", None).unwrap()[0].listening_status
    }
    #[test]
    fn partial_complete_replay_restart_and_backup_keep_distinct_listening_states() {
        let dir = tempdir().unwrap();
        let db = Database::open_and_migrate(&dir.path().join("d"), &dir.path().join("c")).unwrap();
        db.with_conn(|c| { seed(c); Ok(()) }).unwrap();
        assert_eq!(status(&db), ListeningStatus::Unplayed);
        db.save_playback_position("v1", 4000, false, true).unwrap();
        assert_eq!(status(&db), ListeningStatus::InProgress);
        assert_eq!(db.query_smart_collection(&CollectionFilters { listening: ListeningFilter::InProgress, ..Default::default() }).unwrap().len(), 1);
        assert!(db.query_smart_collection(&CollectionFilters { listening: ListeningFilter::Played, ..Default::default() }).unwrap().is_empty());
        db.save_playback_position("v1", 9000, true, true).unwrap();
        assert_eq!(status(&db), ListeningStatus::InProgress);
        db.save_playback_position("v2", 9000, true, true).unwrap();
        assert_eq!(status(&db), ListeningStatus::Played);
        db.save_playback_position("v1", 2000, false, true).unwrap();
        assert_eq!(status(&db), ListeningStatus::Played);
        assert_eq!(db.get_playback_position("v1").unwrap(), (2000, false));
        let bundle = db.export_user_backup().unwrap();
        assert!(bundle.playback_positions.iter().find(|p| p.version_id == "v1").unwrap().ever_completed);
        let restored = Database::open_and_migrate(&dir.path().join("restored"), &dir.path().join("rc")).unwrap();
        restored.with_conn(|c| { seed(c); Ok(()) }).unwrap();
        restored.import_user_backup(&bundle).unwrap();
        assert_eq!(status(&restored), ListeningStatus::Played);
        assert_eq!(restored.get_playback_position("v1").unwrap(), (2000, false));
        drop(db);
        let db = Database::open_and_migrate(&dir.path().join("d"), &dir.path().join("c")).unwrap();
        assert_eq!(status(&db), ListeningStatus::Played);
    }

    #[test]
    fn migration_backfills_completed_history_and_is_not_reapplied() {
        let dir = tempdir().unwrap();
        let data = dir.path().join("d");
        std::fs::create_dir_all(&data).unwrap();
        let conn = Connection::open(data.join("asmr_player.db")).unwrap();
        conn.execute_batch("CREATE TABLE _schema_migrations(version INTEGER PRIMARY KEY,name TEXT NOT NULL,applied_at TEXT NOT NULL);").unwrap();
        for (version, name, sql) in crate::db::MIGRATIONS.iter().filter(|(v, _, _)| *v < 14) {
            conn.execute_batch(sql).unwrap();
            conn.execute("INSERT INTO _schema_migrations VALUES(?1,?2,'now')", params![version, name]).unwrap();
        }
        seed(&conn);
        conn.execute_batch("INSERT INTO playback_positions(version_id,position_ms,completed,updated_at) VALUES('v1',9000,1,'now'),('v2',9000,1,'now');").unwrap();
        drop(conn);
        for _ in 0..2 {
            let db = Database::open_and_migrate(&data, &dir.path().join("c")).unwrap();
            assert_eq!(status(&db), ListeningStatus::Played);
            db.save_playback_position("v1", 0, false, true).unwrap();
        }
    }

    #[test]
    fn version_merge_and_undo_preserve_each_completion_history() {
        let dir = tempdir().unwrap();
        let db = Database::open_and_migrate(&dir.path().join("d"), &dir.path().join("c")).unwrap();
        db.with_conn(|c| { seed(c); Ok(()) }).unwrap();
        db.save_playback_position("v1", 9000, true, true).unwrap();
        db.save_playback_position("v1", 1000, false, true).unwrap();
        db.save_playback_position("v3", 2000, false, true).unwrap();
        let history = db.associate_versions("v3", "v1").unwrap();
        assert!(db.export_user_backup().unwrap().playback_positions.iter().find(|p| p.version_id == "v3").unwrap().ever_completed);
        db.undo_association(&history).unwrap();
        let backup = db.export_user_backup().unwrap();
        assert!(backup.playback_positions.iter().find(|p| p.version_id == "v1").unwrap().ever_completed);
        assert!(!backup.playback_positions.iter().find(|p| p.version_id == "v3").unwrap().ever_completed);
    }
}
