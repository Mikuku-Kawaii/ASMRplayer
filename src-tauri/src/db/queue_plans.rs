//! Named, immutable queue contents. Never resolve missing versions by title.
use super::Database;
use crate::domain::*;
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;
use uuid::Uuid;

fn invalid(message: &str) -> AppError {
    AppError::new(ErrorCode::InvalidInput, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn seed(db: &Database, version: &str, path: &str) {
        db.with_conn(|conn| {
            conn.execute("INSERT OR IGNORE INTO works(id,title,created_at,updated_at) VALUES('w','作品','now','now')",[])?;
            conn.execute("INSERT OR IGNORE INTO tracks(id,work_id,title,sort_key,created_at) VALUES('t','w','同名音轨','1','now')",[])?;
            conn.execute("INSERT INTO track_versions(id,track_id,version_label,created_at) VALUES(?1,'t',?1,'now')",[version])?;
            conn.execute("INSERT INTO media_sources(id,version_id,source_kind,absolute_path,format,created_at,updated_at) VALUES(?1,?1,'local_imported',?2,'mp3','now','now')",params![version,path])?;
            Ok(())
        }).unwrap();
    }
    fn request(data: &QueuePlanPreviewDto, mode: &str, skip: bool) -> ApplyQueuePlanRequest {
        ApplyQueuePlanRequest {
            plan_id: data.plan.id.clone(),
            mode: mode.into(),
            skip_unavailable: skip,
            preview_token: data.preview_token.clone(),
            queue_token: data.queue_token.clone(),
        }
    }

    #[test]
    fn queue_plans_preserve_order_duplicates_sources_names_and_restart() {
        let dir = tempdir().unwrap();
        let data = dir.path().join("data");
        let cache = dir.path().join("cache");
        let audio = dir.path().join("fixture.mp3");
        std::fs::write(&audio, b"fixture").unwrap();
        let db = Database::open_and_migrate(&data, &cache).unwrap();
        assert!(db.save_queue_plan("empty").is_err());
        seed(&db, "original", audio.to_str().unwrap());
        seed(&db, "edit", audio.to_str().unwrap());
        db.append_queue_items(&[
            ("edit".into(), Some("edit".into())),
            ("original".into(), None),
            ("edit".into(), None),
        ])
        .unwrap();
        let plan = db.save_queue_plan("  夜间  ").unwrap();
        assert_eq!(plan.name, "夜间");
        assert_eq!(plan.item_count, 3);
        assert!(db.rename_queue_plan(&plan.id, "\n").is_err());
        db.rename_queue_plan(&plan.id, "睡前队列").unwrap();
        drop(db);
        let db = Database::open_and_migrate(&data, &cache).unwrap();
        assert_eq!(db.list_queue_plans().unwrap()[0].name, "睡前队列");
        let preview = db.preview_queue_plan(&plan.id).unwrap();
        assert!(preview.items.iter().all(|i| i.unavailable_reason.is_none()));
        let current = db
            .apply_queue_plan(&request(&preview, "replace", false), Some("original"))
            .unwrap();
        let queue = db.get_queue_items().unwrap();
        assert_eq!(
            queue
                .iter()
                .map(|i| i.version_id.as_str())
                .collect::<Vec<_>>(),
            vec!["edit", "original", "edit"]
        );
        assert_eq!(queue[0].preferred_source_id.as_deref(), Some("edit"));
        assert_eq!(current.as_deref(), Some(queue[1].id.as_str()));
        let preview = db.preview_queue_plan(&plan.id).unwrap();
        db.apply_queue_plan(&request(&preview, "append", false), None)
            .unwrap();
        assert_eq!(db.get_queue_items().unwrap().len(), 6);
        assert_eq!(db.get_queue_items().unwrap()[1].id, queue[1].id);
    }

    #[test]
    fn queue_plans_require_fresh_preview_and_explicit_skip_without_substitution() {
        let dir = tempdir().unwrap();
        let audio = dir.path().join("test.mp3");
        std::fs::write(&audio, b"fixture").unwrap();
        let db = Database::open_and_migrate(&dir.path().join("d"), &dir.path().join("c")).unwrap();
        seed(&db, "original", audio.to_str().unwrap());
        seed(&db, "edit", audio.to_str().unwrap());
        db.append_queue_items(&[
            ("original".into(), Some("original".into())),
            ("edit".into(), None),
        ])
        .unwrap();
        let plan = db.save_queue_plan("方案").unwrap();
        let before = db.preview_queue_plan(&plan.id).unwrap();
        db.append_queue_items(&[("edit".into(), None)]).unwrap();
        assert!(db
            .apply_queue_plan(&request(&before, "replace", false), None)
            .is_err());
        assert_eq!(db.get_queue_items().unwrap().len(), 3);
        let before = db.preview_queue_plan(&plan.id).unwrap();
        db.with_conn(|conn| {
            conn.execute("DELETE FROM track_versions WHERE id='original'", [])?;
            Ok(())
        })
        .unwrap();
        assert!(db
            .apply_queue_plan(&request(&before, "append", true), None)
            .is_err());
        let preview = db.preview_queue_plan(&plan.id).unwrap();
        assert_eq!(preview.items[0].version_id, "original");
        assert_eq!(preview.items[0].track_title, "同名音轨");
        assert_eq!(
            preview.items[0].unavailable_reason.as_deref(),
            Some("原音轨版本已移除")
        );
        assert!(db
            .apply_queue_plan(&request(&preview, "replace", false), None)
            .is_err());
        db.apply_queue_plan(&request(&preview, "replace", true), None)
            .unwrap();
        assert_eq!(db.get_queue_items().unwrap().len(), 1);
        assert_eq!(db.get_queue_items().unwrap()[0].version_id, "edit");
        // A file disappearing after preview rejects the whole operation, not just that item.
        let preview = db.preview_queue_plan(&plan.id).unwrap();
        std::fs::remove_file(&audio).unwrap();
        assert!(db
            .apply_queue_plan(&request(&preview, "replace", true), None)
            .is_err());
        let empty = db.preview_queue_plan(&plan.id).unwrap();
        assert!(db
            .apply_queue_plan(&request(&empty, "replace", true), None)
            .is_err());
        assert_eq!(db.get_queue_items().unwrap().len(), 1);
    }

    #[test]
    fn queue_plans_reject_partial_files_and_missing_preferred_sources() {
        let dir = tempdir().unwrap();
        let part = dir.path().join("test.MP3.PART");
        std::fs::write(&part, b"unfinished").unwrap();
        let db = Database::open_and_migrate(&dir.path().join("d"), &dir.path().join("c")).unwrap();
        seed(&db, "v", part.to_str().unwrap());
        db.append_queue_items(&[("v".into(), Some("v".into()))])
            .unwrap();
        let plan = db.save_queue_plan("测试").unwrap();
        assert!(db.preview_queue_plan(&plan.id).unwrap().items[0]
            .unavailable_reason
            .is_some());
        db.with_conn(|conn| {
            conn.execute("INSERT INTO media_sources(id,version_id,source_kind,provider,source_work_id,remote_file_id,format,created_at,updated_at) VALUES('online','v','online_provider','asmr_one','1','1','mp3','now','now')",[])?;
            conn.execute("DELETE FROM media_sources WHERE id='v'",[])?; Ok(())
        }).unwrap();
        let preview = db.preview_queue_plan(&plan.id).unwrap();
        assert_eq!(preview.items[0].preferred_source_id.as_deref(), Some("v"));
        assert_eq!(
            preview.items[0].unavailable_reason.as_deref(),
            Some("指定来源不可用")
        );
        assert!(db
            .apply_queue_plan(&request(&preview, "replace", true), None)
            .is_err());
    }

    #[test]
    fn queue_plans_snapshot_the_display_title_and_keep_it_after_metadata_changes() {
        let dir = tempdir().unwrap();
        let db = Database::open_and_migrate(&dir.path().join("d"), &dir.path().join("c")).unwrap();
        seed(&db, "v", "missing.mp3");
        db.append_queue_items(&[("v".into(), None)]).unwrap();
        db.with_conn(|conn| {
            conn.execute("INSERT INTO work_sources(id,work_id,provider,source_work_id,raw_metadata_json,refreshed_at) VALUES('s','w','asmr_one','1',?1,'now')", [r#"{"title":"在线标题"}"#])?;
            Ok(())
        }).unwrap();
        let online = db.save_queue_plan("在线").unwrap();
        assert_eq!(
            db.preview_queue_plan(&online.id).unwrap().items[0].work_title,
            "在线标题"
        );
        db.with_conn(|conn| {
            conn.execute("INSERT INTO work_overrides(work_id,field,override_mode,value_text,updated_at) VALUES('w','title','set','自定义标题','now')", [])?;
            Ok(())
        }).unwrap();
        let custom = db.save_queue_plan("自定义").unwrap();
        assert_eq!(
            db.preview_queue_plan(&custom.id).unwrap().items[0].work_title,
            "自定义标题"
        );
        db.with_conn(|conn| {
            conn.execute(
                "UPDATE work_overrides SET override_mode='cleared' WHERE work_id='w'",
                [],
            )?;
            Ok(())
        })
        .unwrap();
        let cleared = db.save_queue_plan("清空标题").unwrap();
        assert_eq!(
            db.preview_queue_plan(&cleared.id).unwrap().items[0].work_title,
            ""
        );
        assert_eq!(
            db.preview_queue_plan(&custom.id).unwrap().items[0].work_title,
            "自定义标题"
        );
    }

    #[test]
    fn queue_plans_recheck_after_disk_probe_and_rollback_failed_replacement() {
        let dir = tempdir().unwrap();
        let audio = dir.path().join("test.mp3");
        std::fs::write(&audio, b"fixture").unwrap();
        let db = Database::open_and_migrate(&dir.path().join("d"), &dir.path().join("c")).unwrap();
        seed(&db, "v", audio.to_str().unwrap());
        let ids = db
            .append_queue_items(&[("v".into(), Some("v".into()))])
            .unwrap();
        db.update_player_session_row(Some(&ids[0]), 80.0, "queue", 7)
            .unwrap();
        let plan = db.save_queue_plan("测试").unwrap();
        let preview = db.preview_queue_plan(&plan.id).unwrap();
        let prepared = db.prepare_queue_plan(&plan.id).unwrap();
        db.with_conn(|conn| {
            conn.execute(
                "UPDATE media_sources SET availability='missing' WHERE id='v'",
                [],
            )?;
            Ok(())
        })
        .unwrap();
        assert!(db
            .commit_queue_plan(&request(&preview, "replace", false), prepared, Some("v"))
            .is_err());
        db.with_conn(|conn|{
            conn.execute("UPDATE media_sources SET availability='available' WHERE id='v'",[])?;
            conn.execute_batch("CREATE TRIGGER reject_plan_insert BEFORE INSERT ON queue_items BEGIN SELECT RAISE(ABORT,'injected write failure'); END;")?;Ok(())
        }).unwrap();
        assert!(db
            .apply_queue_plan(&request(&preview, "replace", false), Some("v"))
            .is_err());
        assert_eq!(db.get_queue_items().unwrap()[0].id, ids[0]);
        db.with_conn(|conn| {
            let current: String = conn.query_row(
                "SELECT current_queue_item_id FROM player_session WHERE id=1",
                [],
                |r| r.get(0),
            )?;
            assert_eq!(current, ids[0]);
            Ok(())
        })
        .unwrap();
    }
}

fn name(value: &str) -> AppResult<&str> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > 80 || value.chars().any(char::is_control) {
        return Err(invalid("方案名称应为 1–80 字，不能包含控制字符"));
    }
    Ok(value)
}

fn encode<T: serde::Serialize>(value: &T) -> AppResult<String> {
    serde_json::to_string(value).map_err(|_| invalid("队列方案序列化失败"))
}

fn queue_token(conn: &Connection) -> AppResult<String> {
    let mut stmt = conn.prepare(
        "SELECT id,version_id,preferred_source_id FROM queue_items ORDER BY sort_order,id",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    encode(&rows)
}

fn read_plan(conn: &Connection, id: &str) -> AppResult<QueuePlanDto> {
    let (id, name, json, updated_at): (String, String, String, String) = conn
        .query_row(
            "SELECT id,name,items_json,updated_at FROM queue_plans WHERE id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()?
        .ok_or_else(|| invalid("队列方案不存在，请刷新列表"))?;
    let items: Vec<QueuePlanEntryDto> =
        serde_json::from_str(&json).map_err(|_| invalid("队列方案内容损坏"))?;
    Ok(QueuePlanDto {
        id,
        name,
        item_count: items.len(),
        updated_at,
    })
}

type SourceChecks = Vec<(bool, Vec<(String, Option<String>)>)>;

pub(crate) struct PreparedQueuePlan {
    data: QueuePlanPreviewDto,
    source_token: String,
}

// Only read SQLite here. File metadata on disconnected/network drives can block.
fn read_preview(conn: &Connection, id: &str) -> AppResult<(QueuePlanPreviewDto, SourceChecks)> {
    let plan = read_plan(conn, id)?;
    let json: String = conn.query_row(
        "SELECT items_json FROM queue_plans WHERE id=?1",
        [id],
        |r| r.get(0),
    )?;
    let items: Vec<QueuePlanEntryDto> =
        serde_json::from_str(&json).map_err(|_| invalid("队列方案内容损坏"))?;
    let mut checks = Vec::with_capacity(items.len());
    let mut sources = conn.prepare("SELECT source_kind,absolute_path FROM media_sources WHERE version_id=?1 AND availability='available' AND (?2 IS NULL OR id=?2) ORDER BY id")?;
    for item in &items {
        let exists: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM track_versions WHERE id=?1)",
            [&item.version_id],
            |r| r.get(0),
        )?;
        let candidates = sources
            .query_map(params![item.version_id, item.preferred_source_id], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        checks.push((exists, candidates));
    }
    let queue_token = queue_token(conn)?;
    let current_queue_count =
        conn.query_row("SELECT COUNT(*) FROM queue_items", [], |r| r.get(0))?;
    Ok((
        QueuePlanPreviewDto {
            plan,
            items,
            preview_token: String::new(),
            queue_token,
            current_queue_count,
        },
        checks,
    ))
}

impl Database {
    pub fn list_queue_plans(&self) -> AppResult<Vec<QueuePlanDto>> {
        self.with_conn(|conn| {
            let mut stmt =
                conn.prepare("SELECT id FROM queue_plans ORDER BY updated_at DESC,id")?;
            let ids = stmt
                .query_map([], |r| r.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            ids.iter().map(|id| read_plan(conn, id)).collect()
        })
    }

    pub fn save_queue_plan(&self, value: &str) -> AppResult<QueuePlanDto> {
        let name = name(value)?;
        self.with_conn(|conn| {
            let mut stmt = conn.prepare("SELECT q.version_id,q.preferred_source_id,
                COALESCE((SELECT CASE WHEN override_mode='cleared' THEN '' ELSE value_text END
                    FROM work_overrides WHERE work_id=w.id AND field='title'),
                    (SELECT CASE WHEN json_valid(raw_metadata_json) THEN
                        CASE WHEN json_type(raw_metadata_json,'$.title')='text' THEN json_extract(raw_metadata_json,'$.title') END END
                     FROM work_sources WHERE work_id=w.id ORDER BY refreshed_at DESC LIMIT 1),w.title),
                t.title,v.version_label
                FROM queue_items q JOIN track_versions v ON v.id=q.version_id JOIN tracks t ON t.id=v.track_id
                JOIN works w ON w.id=t.work_id ORDER BY q.sort_order,q.id")?;
            let items = stmt.query_map([], |r| Ok(QueuePlanEntryDto {
                version_id:r.get(0)?,preferred_source_id:r.get(1)?,work_title:r.get(2)?,track_title:r.get(3)?,version_label:r.get(4)?,unavailable_reason:None,
            }))?.collect::<Result<Vec<_>, _>>()?;
            if items.is_empty() || items.len() > 10_000 { return Err(invalid("只能保存包含 1–10000 项的队列")); }
            let id = Uuid::new_v4().to_string();
            let now = Utc::now().to_rfc3339();
            conn.execute("INSERT INTO queue_plans(id,name,items_json,created_at,updated_at) VALUES(?1,?2,?3,?4,?4)",params![id,name,encode(&items)?,now])?;
            read_plan(conn, &id)
        })
    }

    pub fn rename_queue_plan(&self, id: &str, value: &str) -> AppResult<QueuePlanDto> {
        let name = name(value)?;
        self.with_conn(|conn| {
            if conn.execute(
                "UPDATE queue_plans SET name=?1,updated_at=?2 WHERE id=?3",
                params![name, Utc::now().to_rfc3339(), id],
            )? == 0
            {
                return Err(invalid("队列方案不存在"));
            }
            read_plan(conn, id)
        })
    }

    pub fn preview_queue_plan(&self, id: &str) -> AppResult<QueuePlanPreviewDto> {
        Ok(self.prepare_queue_plan(id)?.data)
    }

    pub(crate) fn prepare_queue_plan(&self, id: &str) -> AppResult<PreparedQueuePlan> {
        let (mut data, checks) = self.with_conn(|conn| read_preview(conn, id))?;
        let source_token = encode(&(&data.items, &checks))?;
        let mut file_cache = std::collections::HashMap::new();
        for (item, (exists, candidates)) in data.items.iter_mut().zip(checks) {
            let usable = candidates.iter().any(|(kind, path)| {
                kind == "online_provider"
                    || path.as_ref().is_some_and(|p| {
                        *file_cache.entry(p.clone()).or_insert_with(|| {
                            !p.to_lowercase().ends_with(".part") && Path::new(p).is_file()
                        })
                    })
            });
            item.unavailable_reason = if !exists {
                Some("原音轨版本已移除".into())
            } else if !usable && item.preferred_source_id.is_some() {
                Some("指定来源不可用".into())
            } else if !usable {
                Some("没有可用来源（文件缺失或磁盘离线）".into())
            } else {
                None
            };
        }
        data.preview_token = encode(&data.items)?;
        Ok(PreparedQueuePlan { data, source_token })
    }

    #[cfg(test)]
    pub fn apply_queue_plan(
        &self,
        request: &ApplyQueuePlanRequest,
        current_version: Option<&str>,
    ) -> AppResult<Option<String>> {
        let prepared = self.prepare_queue_plan(&request.plan_id)?;
        self.commit_queue_plan(request, prepared, current_version)
    }

    /// Revalidate database identities inside the transaction. Disk checks are outside locks;
    /// disappearance after this check remains a normal, explicit playback source error.
    pub(crate) fn commit_queue_plan(
        &self,
        request: &ApplyQueuePlanRequest,
        prepared: PreparedQueuePlan,
        current_version: Option<&str>,
    ) -> AppResult<Option<String>> {
        if request.mode != "append" && request.mode != "replace" {
            return Err(invalid("未知队列操作"));
        }
        let data = prepared.data;
        if data.plan.id != request.plan_id || data.preview_token != request.preview_token {
            return Err(invalid("方案可用状态已变化，请刷新预览后重试"));
        }
        self.with_conn(|conn| {
            let tx = conn.transaction()?;
            let (fresh,checks) = read_preview(&tx, &request.plan_id)?;
            if encode(&(&fresh.items,&checks))? != prepared.source_token { return Err(invalid("音轨或来源已变化，请刷新预览后重试")); }
            if request.mode == "replace" && fresh.queue_token != request.queue_token { return Err(invalid("当前队列已变化，请刷新预览并重新确认替换")); }
            if !request.skip_unavailable && data.items.iter().any(|i| i.unavailable_reason.is_some()) {
                return Err(invalid("方案含不可用项，请明确选择跳过后再恢复"));
            }
            let items: Vec<_> = data.items.iter().filter(|i| i.unavailable_reason.is_none()).collect();
            if items.is_empty() { return Err(invalid("方案没有可用项，当前队列未改变")); }
            if request.mode == "replace" {
                tx.execute("UPDATE player_session SET current_queue_item_id=NULL WHERE id=1",[])?;
                tx.execute("DELETE FROM queue_items",[])?;
            }
            let start: i64 = tx.query_row("SELECT COALESCE(MAX(sort_order),-1)+1 FROM queue_items",[],|r|r.get(0))?;
            let mut current_id = None;
            for (index,item) in items.iter().enumerate() {
                let id = Uuid::new_v4().to_string();
                tx.execute("INSERT INTO queue_items(id,sort_order,version_id,preferred_source_id,created_at) VALUES(?1,?2,?3,?4,?5)",
                    params![id,start+index as i64,item.version_id,item.preferred_source_id,Utc::now().to_rfc3339()])?;
                if current_id.is_none() && current_version == Some(item.version_id.as_str()) { current_id = Some(id); }
            }
            if request.mode == "replace" {
                tx.execute("UPDATE player_session SET current_queue_item_id=?1,updated_at=?2 WHERE id=1",params![current_id,Utc::now().to_rfc3339()])?;
            }
            tx.commit()?;
            Ok(current_id)
        })
    }
}
