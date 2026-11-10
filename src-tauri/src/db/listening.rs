use crate::domain::{AppResult, ListeningStatus};
use rusqlite::Connection;
use std::collections::HashMap;

pub(super) fn library_states(conn: &Connection, scope: &str) -> AppResult<(HashMap<String, bool>, HashMap<String, ListeningStatus>)> {
    let mut listened = HashMap::new();
    let mut result = HashMap::new();
    // A complete directory is one listening edition; do not require duplicate
    // WAV/MP3 or alternate edits to be played as well.
    // T1515: Join history once for both UI flags. Collapse alternate versions per
    // track before counting completion; unheard/versionless tracks stay in the denominator.
    let mut stmt = conn.prepare("WITH track_history AS (
        SELECT t.work_id,t.logical_dir_path,t.id,
               MAX(p.position_ms>0 OR p.completed=1) AS listened,
               MAX(p.ever_completed) AS heard
        FROM json_each(?1) selected CROSS JOIN tracks t ON t.work_id=selected.value
        CROSS JOIN track_versions v ON v.track_id=t.id
        CROSS JOIN playback_positions p ON p.version_id=v.id GROUP BY t.id
    ) SELECT c.work_id,MAX(c.listened),MAX(c.listened OR c.heard),
             SUM(c.heard)=(SELECT COUNT(*) FROM tracks t
                          WHERE t.work_id=c.work_id AND t.logical_dir_path=c.logical_dir_path)
      FROM track_history c
      GROUP BY c.work_id,c.logical_dir_path
    ")?;
    for row in stmt.query_map([scope], |r| Ok((r.get::<_, String>(0)?, r.get::<_, bool>(1)?, r.get::<_, bool>(2)?, r.get::<_, bool>(3)?)))? {
        let (id, has_listened, started, complete) = row?;
        if has_listened { listened.insert(id.clone(), true); }
        if complete { result.insert(id, ListeningStatus::Played); }
        else if started { result.entry(id).or_insert(ListeningStatus::InProgress); }
    }
    let mut stmt = conn.prepare("SELECT work_id FROM work_playback_state WHERE work_id IN (SELECT value FROM json_each(?1))")?;
    for row in stmt.query_map([scope], |r| r.get::<_, String>(0))? {
        let id = row?;
        listened.insert(id.clone(), true);
        result.entry(id).or_insert(ListeningStatus::InProgress);
    }
    let mut stmt = conn.prepare("SELECT work_id,listened FROM work_listening_marks WHERE work_id IN (SELECT value FROM json_each(?1))")?;
    for row in stmt.query_map([scope], |r| Ok((r.get::<_, String>(0)?, r.get::<_, bool>(1)?)))? {
        let (id, value) = row?;
        listened.insert(id.clone(), value);
        result.insert(id, if value { ListeningStatus::Played } else { ListeningStatus::Unplayed });
    }
    Ok((listened, result))
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
    fn t1515_completed_versions_do_not_replace_unheard_or_versionless_tracks() {
        let dir = tempdir().unwrap();
        let db = Database::open_and_migrate(&dir.path().join("d"), &dir.path().join("c")).unwrap();
        db.with_conn(|c| {
            seed(c);
            c.execute_batch("INSERT INTO track_versions(id,track_id,version_label,created_at) VALUES('v1-alt','t1','Alternate','now');")?;
            Ok(())
        }).unwrap();
        db.save_playback_position("v1", 9000, true, true).unwrap();
        db.save_playback_position("v1-alt", 9000, true, true).unwrap();
        assert_eq!(status(&db), ListeningStatus::InProgress);
        db.save_playback_position("v2", 9000, true, true).unwrap();
        assert_eq!(status(&db), ListeningStatus::Played);
        db.with_conn(|c| {
            c.execute_batch("INSERT INTO tracks(id,work_id,title,logical_dir_path,sort_key,created_at) VALUES('t4','w','No version','WAV','4','now');")?;
            Ok(())
        }).unwrap();
        assert_eq!(status(&db), ListeningStatus::InProgress);
        db.save_playback_position("v3", 9000, true, true).unwrap();
        assert_eq!(status(&db), ListeningStatus::Played); // the alternate MP3 directory is complete
        db.with_conn(|c| {
            c.execute_batch("INSERT INTO work_listening_marks(work_id,listened,updated_at) VALUES('w',0,'now');")?;
            Ok(())
        }).unwrap();
        assert_eq!(status(&db), ListeningStatus::Unplayed); // explicit user mark wins
        assert!(!db.list_works("all", None).unwrap()[0].is_listened);
        db.with_conn(|c| {
            c.execute_batch("DELETE FROM work_listening_marks; DELETE FROM work_playback_state;
                UPDATE playback_positions SET position_ms=0,completed=0;")?;
            Ok(())
        }).unwrap();
        let summary = db.list_works("all", None).unwrap().remove(0);
        assert_eq!(summary.listening_status, ListeningStatus::Played);
        assert!(!summary.is_listened); // legacy progress flag differs from durable completion
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
