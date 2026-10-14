//! T1503: version-bound notes; unresolved imports never guess a version from titles.
use super::Database;
use crate::domain::*;
use chrono::Utc;
use rusqlite::{params, Connection};
use std::collections::HashSet;
use uuid::Uuid;

fn json<T: serde::Serialize>(value: &T) -> AppResult<String> {
    serde_json::to_string(value).map_err(|_| invalid("书签序列化失败"))
}

fn invalid(message: &str) -> AppError {
    AppError::new(ErrorCode::InvalidInput, message)
}

pub(super) fn normalized_tags(note: &str, tags: &[String]) -> AppResult<Vec<String>> {
    if note.chars().count() > 10_000 || note.contains('\0') || tags.len() > 16 {
        return Err(invalid("笔记最多 10000 字，标签最多 16 个，不能包含空字符"));
    }
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    for tag in tags {
        let tag = tag.trim();
        if tag.is_empty() || tag.chars().count() > 64 || tag.chars().any(char::is_control) {
            return Err(invalid("每个标签应为 1–64 字，不能包含控制字符"));
        }
        if seen.insert(tag.to_lowercase()) {
            result.push(tag.to_string());
        }
    }
    Ok(result)
}

pub(super) fn read_records(conn: &Connection) -> AppResult<Vec<BackupBookmarkRecordDto>> {
    let mut stmt = conn.prepare(
        "SELECT b.id,b.version_id,w.rj_code,t.title,b.position_ms,b.note,b.created_at,
         b.tags_json,t.work_id,COALESCE((SELECT CASE WHEN override_mode='cleared' THEN '' ELSE value_text END
         FROM work_overrides WHERE work_id=w.id AND field='title'),
         (SELECT CASE WHEN json_valid(raw_metadata_json) THEN
             CASE WHEN json_type(raw_metadata_json,'$.title')='text' THEN json_extract(raw_metadata_json,'$.title') END END
          FROM work_sources WHERE work_id=w.id ORDER BY refreshed_at DESC LIMIT 1),w.title),tv.version_label,b.updated_at
         FROM bookmarks b JOIN track_versions tv ON tv.id=b.version_id
         JOIN tracks t ON t.id=tv.track_id JOIN works w ON w.id=t.work_id")?;
    let mut records = stmt
        .query_map([], |r| {
            let tags: String = r.get(7)?;
            let tags = serde_json::from_str(&tags).map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    7,
                    rusqlite::types::Type::Text,
                    Box::new(e),
                )
            })?;
            Ok(BackupBookmarkRecordDto {
                id: r.get(0)?,
                version_id: r.get(1)?,
                rj_code: r.get(2)?,
                track_title: r.get(3)?,
                position_ms: r.get(4)?,
                note: r.get(5)?,
                created_at: r.get(6)?,
                tags,
                work_id: r.get(8)?,
                work_title: r.get(9)?,
                version_label: r.get(10)?,
                updated_at: r.get(11)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut orphan_stmt = conn.prepare("SELECT record_json FROM unlinked_bookmarks ORDER BY id")?;
    for row in orphan_stmt.query_map([], |r| r.get::<_, String>(0))? {
        records.push(serde_json::from_str(&row?).map_err(|_| invalid("未关联书签记录损坏"))?);
    }
    records.sort_by(|a, b| b.created_at.cmp(&a.created_at).then(a.id.cmp(&b.id)));
    Ok(records)
}

pub(crate) fn validate_records(records: &[BackupBookmarkRecordDto]) -> AppResult<()> {
    if records.len() > 10_000 {
        return Err(invalid("一次最多恢复 10000 个书签"));
    }
    let mut ids = HashSet::new();
    for bm in records {
        if bm.id.is_empty()
            || bm.id.len() > 200
            || bm.version_id.is_empty()
            || bm.version_id.len() > 200
            || bm.position_ms < 0
            || bm.position_ms > 31_536_000_000
            || !ids.insert(&bm.id)
            || [
                &bm.work_title,
                &bm.track_title,
                &bm.version_label,
                &bm.work_id,
            ]
            .iter()
            .any(|s| s.len() > 4096)
        {
            return Err(invalid("备份中存在重复书签、无效版本、位置或过长标题"));
        }
        normalized_tags(&bm.note, &bm.tags)?;
    }
    Ok(())
}

// Only insert missing records. A backup must never overwrite notes edited since export.
pub(super) fn restore_records(
    conn: &Connection,
    records: &[BackupBookmarkRecordDto],
    apply: bool,
) -> AppResult<BookmarkRestoreReport> {
    validate_records(records)?;
    let mut report = BookmarkRestoreReport {
        restored: 0,
        unlinked: 0,
        skipped: 0,
    };
    for bm in records {
        let exists: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM bookmarks WHERE id=?1 UNION ALL SELECT 1 FROM unlinked_bookmarks WHERE id=?1)", [&bm.id], |r| r.get(0))?;
        if exists {
            report.skipped += 1;
            continue;
        }
        let linked: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM track_versions tv JOIN tracks t ON t.id=tv.track_id WHERE tv.id=?1 AND (?2='' OR t.work_id=?2))", params![bm.version_id,bm.work_id], |r| r.get(0))?;
        report.restored += 1;
        if !linked {
            report.unlinked += 1;
        }
        if !apply {
            continue;
        }
        let mut record = bm.clone();
        record.tags = normalized_tags(&bm.note, &bm.tags)?;
        if linked {
            conn.execute("INSERT INTO bookmarks(id,version_id,position_ms,note,created_at,updated_at,tags_json) VALUES (?1,?2,?3,?4,?5,?6,?7)",
                params![record.id,record.version_id,record.position_ms,record.note,record.created_at,
                    if record.updated_at.is_empty() { &record.created_at } else { &record.updated_at },json(&record.tags)?])?;
        } else {
            conn.execute(
                "INSERT INTO unlinked_bookmarks(id,record_json) VALUES (?1,?2)",
                params![record.id, json(&record)?],
            )?;
        }
    }
    Ok(report)
}

impl Database {
    pub fn search_bookmarks(
        &self,
        work_id: Option<&str>,
        version_id: Option<&str>,
        query: &str,
        offset: usize,
    ) -> AppResult<BookmarkSearchPage> {
        if query.chars().count() > 200 {
            return Err(invalid("检索词最多 200 字"));
        }
        let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
        let (mut page, local_paths) = self.with_conn(|conn| {
            let records: Vec<_> = read_records(conn)?.into_iter().filter(|b|
                work_id.is_none_or(|id| b.work_id==id) && version_id.is_none_or(|id| b.version_id==id)
                && { let haystack=format!("{} {} {} {} {} {}",b.note,b.tags.join(" "),b.work_title,b.track_title,b.version_label,b.rj_code.as_deref().unwrap_or("")).to_lowercase(); words.iter().all(|word| haystack.contains(word)) }
            ).collect();
            let total=records.len();
            let mut items=Vec::new();
            let mut local_paths = Vec::new();
            for bm in records.into_iter().skip(offset).take(100) {
                let linked: bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM bookmarks WHERE id=?1)",[&bm.id],|r|r.get(0))?;
                let mut playable=false;
                if linked {
                    let mut sources=conn.prepare("SELECT source_kind,absolute_path FROM media_sources WHERE version_id=?1 AND availability='available'")?;
                    for source in sources.query_map([&bm.version_id], |r|Ok((r.get::<_,String>(0)?,r.get::<_,Option<String>>(1)?)))? {
                        let (kind,path)=source?;
                        if kind=="online_provider" { playable=true;break; }
                        if let Some(path) = path { local_paths.push((items.len(), path)); }
                    }
                }
                items.push(BookmarkDto { id:bm.id,version_id:bm.version_id,track_title:bm.track_title,work_id:bm.work_id,position_ms:bm.position_ms,note:bm.note,
                    created_at:bm.created_at,tags:bm.tags,work_title:bm.work_title,version_label:bm.version_label,linked,playable });
            }
            Ok((BookmarkSearchPage {items,total}, local_paths))
        })?;
        // Disk checks can block on detached/network volumes; never hold the database lock.
        for (index, path) in local_paths {
            if !page.items[index].playable
                && !path.to_lowercase().ends_with(".part")
                && std::path::Path::new(&path).is_file()
            {
                page.items[index].playable = true;
            }
        }
        Ok(page)
    }

    pub fn list_bookmarks(
        &self,
        work_id: Option<&str>,
        version_id: Option<&str>,
    ) -> AppResult<Vec<BookmarkDto>> {
        let mut result = Vec::new();
        loop {
            let page = self.search_bookmarks(work_id, version_id, "", result.len())?;
            result.extend(page.items);
            if result.len() >= page.total {
                break;
            }
        }
        result.sort_by_key(|b| b.position_ms);
        Ok(result)
    }

    pub fn get_bookmark(&self, id: &str) -> AppResult<BackupBookmarkRecordDto> {
        self.with_conn(|conn| {
            let linked: bool = conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM bookmarks WHERE id=?1)",
                [id],
                |r| r.get(0),
            )?;
            if !linked {
                return Err(invalid("书签未关联到原音轨版本，笔记仍可查看"));
            }
            read_records(conn)?
                .into_iter()
                .find(|b| b.id == id)
                .ok_or_else(|| invalid("书签不存在或已移除"))
        })
    }

    pub fn add_bookmark(
        &self,
        version_id: &str,
        position_ms: i64,
        note: &str,
    ) -> AppResult<BookmarkDto> {
        self.add_bookmark_note(version_id, position_ms, note, &[])
    }

    pub fn add_bookmark_note(
        &self,
        version_id: &str,
        position_ms: i64,
        note: &str,
        tags: &[String],
    ) -> AppResult<BookmarkDto> {
        let tags = normalized_tags(note, tags)?;
        if !(0..=31_536_000_000).contains(&position_ms) {
            return Err(invalid("书签位置超出有效范围"));
        }
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        self.with_conn(|conn| {
            conn.execute("INSERT INTO bookmarks(id,version_id,position_ms,note,tags_json,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,?6,?6)",params![id,version_id,position_ms,note,json(&tags)?,now])?;
            Ok(())
        })?;
        self.list_bookmarks(None, Some(version_id))?
            .into_iter()
            .find(|b| b.id == id)
            .ok_or_else(|| invalid("书签保存后无法读取"))
    }

    pub fn update_bookmark_note(
        &self,
        id: &str,
        note: &str,
        tags: Option<&[String]>,
    ) -> AppResult<()> {
        self.with_conn(|conn| {
            let mut bm = read_records(conn)?
                .into_iter()
                .find(|b| b.id == id)
                .ok_or_else(|| invalid("书签不存在或已移除"))?;
            bm.tags = normalized_tags(note, tags.unwrap_or(&bm.tags))?;
            bm.note = note.to_string();
            bm.updated_at = Utc::now().to_rfc3339();
            if conn.execute(
                "UPDATE bookmarks SET note=?1,tags_json=?2,updated_at=?3 WHERE id=?4",
                params![bm.note, json(&bm.tags)?, bm.updated_at, id],
            )? == 0
            {
                conn.execute(
                    "UPDATE unlinked_bookmarks SET record_json=?1 WHERE id=?2",
                    params![json(&bm)?, id],
                )?;
            }
            Ok(())
        })
    }

    pub fn delete_bookmark(&self, id: &str) -> AppResult<()> {
        self.with_conn(|conn| {
            let tx = conn.transaction()?;
            tx.execute("DELETE FROM bookmarks WHERE id=?1", [id])?;
            tx.execute("DELETE FROM unlinked_bookmarks WHERE id=?1", [id])?;
            tx.commit()?;
            Ok(())
        })
    }

    pub fn export_bookmark_notes(&self) -> AppResult<BookmarkBundle> {
        self.with_conn(|conn| {
            Ok(BookmarkBundle {
                schema_version: 1,
                bookmarks: read_records(conn)?,
            })
        })
    }

    pub fn restore_bookmark_notes(
        &self,
        bundle: &BookmarkBundle,
        apply: bool,
    ) -> AppResult<BookmarkRestoreReport> {
        if bundle.schema_version != 1 {
            return Err(invalid("不支持此书签备份版本"));
        }
        self.with_conn(|conn| {
            let tx = conn.transaction()?;
            let report = restore_records(&tx, &bundle.bookmarks, false)?;
            if apply && report.restored > 0 {
                // Before any change, retain an exclusive, local snapshot. Failure aborts restoration.
                let backup = BookmarkBundle {
                    schema_version: 1,
                    bookmarks: read_records(&tx)?,
                };
                use std::io::Write;
                let path = self.data_dir.join(format!(
                    "bookmark-notes-before-restore-{}.json",
                    Uuid::new_v4()
                ));
                let mut file = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(path)?;
                file.write_all(json(&backup)?.as_bytes())?;
                file.sync_all()?;
                restore_records(&tx, &bundle.bookmarks, true)?;
            }
            tx.commit()?;
            Ok(report)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    fn seed(db: &Database, work: &str, version: &str) {
        db.with_conn(|conn| {
            conn.execute("INSERT OR IGNORE INTO works(id,title,rj_code,created_at,updated_at) VALUES (?1,?1,'RJ0001','now','now')",[work])?;
            conn.execute("INSERT INTO tracks(id,work_id,title,sort_key,created_at) VALUES (?1,?2,'同名音轨','1','now')",params![version,work])?;
            conn.execute("INSERT INTO track_versions(id,track_id,version_label,created_at) VALUES (?1,?1,?1,'now')",[version])?;
            Ok(())
        }).unwrap();
    }
    #[test]
    fn titles_follow_metadata_priority_and_search_rj_t1503() {
        let dir = tempdir().unwrap();
        let db = Database::open_and_migrate(&dir.path().join("d"), &dir.path().join("c")).unwrap();
        seed(&db, "local", "v");
        db.add_bookmark("v", 0, "note").unwrap();
        db.with_conn(|conn| {
            conn.execute("INSERT INTO work_sources(id,work_id,provider,source_work_id,raw_metadata_json,refreshed_at) VALUES('s','local','asmr_one','1','{\"title\":\"线上标题\"}','now')", [])?;
            Ok(())
        }).unwrap();
        assert_eq!(
            db.search_bookmarks(None, None, "线上标题 RJ0001", 0)
                .unwrap()
                .total,
            1
        );
        db.with_conn(|conn| {
            conn.execute("INSERT INTO work_overrides(work_id,field,value_text,override_mode,updated_at) VALUES('local','title','自定义标题','set','now')", [])?;
            Ok(())
        }).unwrap();
        assert_eq!(
            db.search_bookmarks(None, None, "自定义标题", 0)
                .unwrap()
                .total,
            1
        );
        assert_eq!(
            db.search_bookmarks(None, None, "线上标题", 0)
                .unwrap()
                .total,
            0
        );
    }

    #[test]
    fn notes_search_missing_source_and_persistent_edits_t1503() {
        let dir = tempdir().unwrap();
        let data = dir.path().join("d");
        let cache = dir.path().join("c");
        let db = Database::open_and_migrate(&data, &cache).unwrap();
        seed(&db, "作品甲", "original");
        seed(&db, "作品乙", "edit");
        let first = db
            .add_bookmark_note(
                "original",
                12345,
                "雨声\n轻声细语",
                &[" 助眠 ".into(), "ASMR".into(), "asmr".into()],
            )
            .unwrap();
        db.add_bookmark("edit", 5000, "未匹配内容").unwrap();
        assert!(!first.playable);
        assert!(first.linked);
        assert_eq!(first.tags, vec!["助眠", "ASMR"]);
        let result = db
            .search_bookmarks(None, None, "作品甲 asmr 雨声", 0)
            .unwrap();
        assert_eq!(result.total, 1);
        assert!(db
            .search_bookmarks(None, Some("edit"), "雨声", 0)
            .unwrap()
            .items
            .is_empty());
        db.update_bookmark_note(&first.id, "修改后的笔记", Some(&["睡眠".into()]))
            .unwrap();
        assert_eq!(db.search_bookmarks(None, None, "睡眠", 0).unwrap().total, 1);
        drop(db);
        let db = Database::open_and_migrate(&data, &cache).unwrap();
        assert_eq!(db.get_bookmark(&first.id).unwrap().note, "修改后的笔记");
        assert_eq!(db.get_bookmark(&first.id).unwrap().position_ms, 12345);
        assert!(db
            .update_bookmark_note(&first.id, "bad", Some(&["\0".into()]))
            .is_err());
        assert!(db.update_bookmark_note("missing", "note", None).is_err());
    }
    #[test]
    fn restore_keeps_exact_versions_and_unlinked_notes_without_overwrites_t1503() {
        let dir = tempdir().unwrap();
        let db = Database::open_and_migrate(&dir.path().join("a"), &dir.path().join("ac")).unwrap();
        seed(&db, "原作品", "original");
        let bm = db
            .add_bookmark_note("original", 1200, "珍贵笔记", &["雨声".into()])
            .unwrap();
        let bundle = db.export_bookmark_notes().unwrap();
        let other =
            Database::open_and_migrate(&dir.path().join("b"), &dir.path().join("bc")).unwrap();
        seed(&other, "原作品", "different-cut");
        let preview = other.restore_bookmark_notes(&bundle, false).unwrap();
        assert_eq!(preview.unlinked, 1);
        assert!(other.list_bookmarks(None, None).unwrap().is_empty());
        let report = other.restore_bookmark_notes(&bundle, true).unwrap();
        assert_eq!(report.restored, 1);
        let notes = other.list_bookmarks(None, None).unwrap();
        assert!(!notes[0].linked);
        assert!(!notes[0].playable);
        assert_eq!(notes[0].version_id, "original");
        assert!(other.get_bookmark(&bm.id).is_err());
        other
            .update_bookmark_note(&bm.id, "离线编辑", Some(&["保留".into()]))
            .unwrap();
        assert_eq!(
            other.restore_bookmark_notes(&bundle, true).unwrap().skipped,
            1
        );
        let saved = other.export_bookmark_notes().unwrap();
        assert_eq!(saved.bookmarks[0].note, "离线编辑");
        assert_eq!(saved.bookmarks[0].tags, vec!["保留"]);
        let third =
            Database::open_and_migrate(&dir.path().join("c"), &dir.path().join("cc")).unwrap();
        seed(&third, "原作品", "original");
        assert_eq!(
            third.restore_bookmark_notes(&saved, true).unwrap().unlinked,
            0
        );
        assert_eq!(third.get_bookmark(&bm.id).unwrap().position_ms, 1200);
        assert_eq!(
            std::fs::read_dir(other.data_dir.clone())
                .unwrap()
                .filter_map(Result::ok)
                .filter(|e| e
                    .file_name()
                    .to_string_lossy()
                    .starts_with("bookmark-notes-before-"))
                .count(),
            1
        );
        other.delete_bookmark(&bm.id).unwrap();
        assert!(other.list_bookmarks(None, None).unwrap().is_empty());
    }
    #[test]
    fn restore_is_atomic_and_validates_whole_bundle_t1503() {
        let dir = tempdir().unwrap();
        let db = Database::open_and_migrate(&dir.path().join("d"), &dir.path().join("c")).unwrap();
        seed(&db, "作品", "v");
        db.add_bookmark("v", 0, "一").unwrap();
        db.add_bookmark("v", 1, "二").unwrap();
        let mut bundle = db.export_bookmark_notes().unwrap();
        for bm in &bundle.bookmarks {
            db.delete_bookmark(&bm.id).unwrap();
        }
        let mut invalid = bundle.clone();
        invalid.bookmarks[1].position_ms = -1;
        assert!(db.restore_bookmark_notes(&invalid, true).is_err());
        assert!(db.list_bookmarks(None, None).unwrap().is_empty());
        db.with_conn(|conn| {conn.execute_batch("CREATE TRIGGER fail_notes BEFORE INSERT ON bookmarks WHEN NEW.note='二' BEGIN SELECT RAISE(ABORT,'test failure'); END;")?;Ok(())}).unwrap();
        assert!(db.restore_bookmark_notes(&bundle, true).is_err());
        assert!(db.list_bookmarks(None, None).unwrap().is_empty());
        bundle.schema_version = 999;
        assert!(db.restore_bookmark_notes(&bundle, false).is_err());
    }
    #[test]
    fn full_backup_preserves_tags_and_legacy_defaults_t1503() {
        let dir = tempdir().unwrap();
        let db = Database::open_and_migrate(&dir.path().join("d"), &dir.path().join("c")).unwrap();
        seed(&db, "作品", "v");
        let bm = db
            .add_bookmark_note("v", 999, "notes", &["标签".into()])
            .unwrap();
        let bundle = db.export_user_backup().unwrap();
        db.delete_bookmark(&bm.id).unwrap();
        db.import_user_backup(&bundle).unwrap();
        assert_eq!(db.get_bookmark(&bm.id).unwrap().tags, vec!["标签"]);
        let legacy:BackupBookmarkRecordDto=serde_json::from_str(r#"{"id":"old","versionId":"v","rjCode":null,"trackTitle":"track","positionMs":2,"note":"old note","createdAt":"now"}"#).unwrap();
        assert!(legacy.tags.is_empty());
        assert!(legacy.work_id.is_empty());
        db.restore_bookmark_notes(
            &BookmarkBundle {
                schema_version: 1,
                bookmarks: vec![legacy],
            },
            true,
        )
        .unwrap();
        assert_eq!(db.get_bookmark("old").unwrap().version_id, "v");
    }
    #[test]
    fn pagination_filters_all_results_and_ignores_part_files_t1503() {
        let dir = tempdir().unwrap();
        let db = Database::open_and_migrate(&dir.path().join("d"), &dir.path().join("c")).unwrap();
        seed(&db, "作品", "v");
        db.with_conn(|conn| {
            for n in 0..105 {conn.execute("INSERT INTO bookmarks(id,version_id,position_ms,note,created_at,updated_at) VALUES (?1,'v',?2,?1,'now','now')",params![format!("note-{n:03}"),n])?;}
            let path=dir.path().join("unfinished.part");std::fs::write(&path,b"test")?;
            conn.execute("INSERT INTO media_sources(id,version_id,source_kind,absolute_path,format,created_at,updated_at) VALUES('s','v','local_imported',?1,'wav','now','now')",[path.to_string_lossy().as_ref()])?;Ok(())
        }).unwrap();
        let page = db.search_bookmarks(None, None, "", 0).unwrap();
        assert_eq!(page.total, 105);
        assert_eq!(page.items.len(), 100);
        assert!(!page.items[0].playable);
        assert_eq!(
            db.search_bookmarks(None, None, "", 100)
                .unwrap()
                .items
                .len(),
            5
        );
        assert_eq!(
            db.search_bookmarks(None, None, "note-104", 0)
                .unwrap()
                .total,
            1
        );
    }
}
