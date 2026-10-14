use super::Database;
use crate::domain::{AppError, AppResult, ErrorCode};
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaAffinity {
    pub work_id: String,
    pub version_id: String,
    pub is_collected: bool,
    pub is_favorite: bool,
    pub is_version_favorite: bool,
    pub is_local_imported: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    fn seed(db: &Database) {
        db.with_conn(|c| { c.execute_batch("INSERT INTO works(id,title,created_at,updated_at) VALUES('w','Work','now','now');
          INSERT INTO tracks(id,work_id,title,logical_dir_path,sort_key,created_at) VALUES('t','w','Track','','1','now');
          INSERT INTO track_versions(id,track_id,version_label,created_at) VALUES('a','t','A','now'),('b','t','B','now');
          INSERT INTO media_sources(id,version_id,source_kind,format,created_at,updated_at) VALUES('s','a','local_imported','wav','now','now');")?; Ok(()) }).unwrap();
    }
    #[test]
    fn track_favorites_are_version_bound_durable_and_add_parent_work() {
        let dir=tempdir().unwrap(); let db=Database::open_and_migrate(&dir.path().join("d"),&dir.path().join("c")).unwrap(); seed(&db);
        assert!(!db.media_affinity("a").unwrap().is_version_favorite);
        let a=db.toggle_version_favorite("a").unwrap(); assert!(a.is_favorite && a.is_version_favorite && a.is_local_imported);
        assert!(!db.media_affinity("b").unwrap().is_version_favorite);
        assert_eq!(db.list_works("favorites",None).unwrap().len(),1);
        let backup=db.export_user_backup().unwrap(); assert_eq!(backup.favorite_versions.len(),1);
        assert!(db.toggle_version_favorite("missing").is_err());
        let a=db.toggle_version_favorite("a").unwrap(); assert!(!a.is_version_favorite && a.is_favorite);
        db.import_user_backup(&backup).unwrap(); db.import_user_backup(&backup).unwrap();
        assert_eq!(db.export_user_backup().unwrap().favorite_versions.len(),1);
        drop(db);
        let db=Database::open_and_migrate(&dir.path().join("d"),&dir.path().join("c")).unwrap(); assert!(db.media_affinity("a").unwrap().is_version_favorite);
    }
    #[test]
    fn version_merge_and_undo_preserve_individual_favorites() {
        let dir=tempdir().unwrap(); let db=Database::open_and_migrate(&dir.path().join("d"),&dir.path().join("c")).unwrap(); seed(&db);
        db.toggle_version_favorite("a").unwrap();
        let history=db.associate_versions("b","a").unwrap();
        assert!(db.media_affinity("b").unwrap().is_version_favorite);
        db.undo_association(&history).unwrap();
        assert!(db.media_affinity("a").unwrap().is_version_favorite);
        assert!(!db.media_affinity("b").unwrap().is_version_favorite);
    }
}
fn read(conn: &Connection, version: &str) -> AppResult<MediaAffinity> {
    conn.query_row("SELECT t.work_id,
        EXISTS(SELECT 1 FROM collected_works WHERE work_id=t.work_id),
        EXISTS(SELECT 1 FROM favorites WHERE work_id=t.work_id),
        EXISTS(SELECT 1 FROM favorite_versions WHERE version_id=v.id),
        EXISTS(SELECT 1 FROM tracks lt JOIN track_versions lv ON lv.track_id=lt.id JOIN media_sources s ON s.version_id=lv.id WHERE lt.work_id=t.work_id AND s.source_kind='local_imported')
        FROM track_versions v JOIN tracks t ON t.id=v.track_id WHERE v.id=?1", [version], |r| Ok(MediaAffinity {
            work_id: r.get(0)?, version_id: version.into(), is_collected: r.get(1)?, is_favorite: r.get(2)?, is_version_favorite: r.get(3)?, is_local_imported: r.get(4)?,
        })).optional()?.ok_or_else(|| AppError::new(ErrorCode::SourceNotFound, "音轨版本已不存在"))
}
impl Database {
    pub fn media_affinity(&self, version_id: &str) -> AppResult<MediaAffinity> {
        self.with_conn(|conn| read(conn, version_id))
    }
    pub fn toggle_version_favorite(&self, version_id: &str) -> AppResult<MediaAffinity> {
        self.with_conn(|conn| {
            let tx = conn.transaction()?;
            let before = read(&tx, version_id)?;
            if before.is_version_favorite {
                tx.execute("DELETE FROM favorite_versions WHERE version_id=?1", [version_id])?;
            } else {
                let now = Utc::now().to_rfc3339();
                tx.execute("INSERT INTO favorite_versions(version_id,created_at) VALUES(?1,?2)", params![version_id,now])?;
                tx.execute("INSERT OR IGNORE INTO favorites(work_id,created_at) VALUES(?1,?2)", params![before.work_id,now])?;
            }
            let result = read(&tx, version_id)?;
            tx.commit()?;
            Ok(result)
        })
    }
}
