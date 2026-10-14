use super::Database;
use crate::domain::{AppError, AppResult, ErrorCode};
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use uuid::Uuid;

pub(super) fn listening_states(
    conn: &Connection,
) -> AppResult<std::collections::HashMap<String, bool>> {
    let mut states = std::collections::HashMap::new();
    let mut stmt = conn.prepare("SELECT work_id FROM work_playback_state UNION SELECT t.work_id FROM tracks t JOIN track_versions v ON v.track_id=t.id JOIN playback_positions p ON p.version_id=v.id WHERE p.position_ms>0 OR p.completed=1")?;
    for id in stmt.query_map([], |r| r.get::<_, String>(0))? {
        states.insert(id?, true);
    }
    let mut stmt = conn.prepare("SELECT work_id,listened FROM work_listening_marks")?;
    for row in stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, bool>(1)?)))? {
        let (id, value) = row?;
        states.insert(id, value);
    }
    Ok(states)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum BulkAction {
    Collected { value: bool },
    Favorite { value: bool },
    AddTags { tags: Vec<String> },
    RemoveTags { tags: Vec<String> },
    Listened { value: bool },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BulkItem {
    pub work_id: String,
    pub title: String,
    pub status: String,
    pub error: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BulkReport {
    pub id: String,
    pub action: BulkAction,
    pub phase: String,
    pub items: Vec<BulkItem>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct FieldState {
    // Original row, including timestamps, is retained so undo restores an override's absence too.
    row: Option<Vec<String>>,
    tags: Vec<String>,
    flag: bool,
}
#[derive(Clone, Serialize, Deserialize)]
struct Entry {
    item: BulkItem,
    before: Option<FieldState>,
    after: Option<FieldState>,
}
#[derive(Clone, Serialize, Deserialize)]
struct Journal {
    id: String,
    action: BulkAction,
    phase: String,
    entries: Vec<Entry>,
}
impl Journal {
    fn report(&self) -> BulkReport {
        BulkReport {
            id: self.id.clone(),
            action: self.action.clone(),
            phase: self.phase.clone(),
            items: self.entries.iter().map(|e| e.item.clone()).collect(),
        }
    }
}
fn invalid(message: &str) -> AppError {
    AppError::new(ErrorCode::InvalidInput, message)
}
fn json<T: Serialize>(value: &T) -> AppResult<String> {
    serde_json::to_string(value).map_err(|_| invalid("无法保存批量整理记录"))
}
fn load(conn: &Connection, id: &str) -> AppResult<Journal> {
    let value: Option<String> = conn
        .query_row(
            "SELECT journal_json FROM library_batches WHERE id=?1",
            [id],
            |r| r.get(0),
        )
        .optional()?;
    serde_json::from_str(&value.ok_or_else(|| invalid("批量预览已失效，请重新预览"))?)
        .map_err(|_| invalid("批量记录无法读取"))
}
fn store(conn: &Connection, journal: &Journal) -> AppResult<()> {
    conn.execute(
        "UPDATE library_batches SET journal_json=?1 WHERE id=?2",
        params![json(journal)?, journal.id],
    )?;
    Ok(())
}
fn latest_id(conn: &Connection) -> AppResult<Option<String>> {
    Ok(conn.query_row("SELECT id FROM library_batches WHERE execution_seq IS NOT NULL ORDER BY execution_seq DESC LIMIT 1", [], |r| r.get(0)).optional()?)
}
fn normalize(action: BulkAction) -> AppResult<BulkAction> {
    let clean = |tags: Vec<String>| -> AppResult<Vec<String>> {
        if tags.is_empty() || tags.len() > 16 {
            return Err(invalid("每次调整需填写 1–16 个标签"));
        }
        let mut seen = HashSet::new();
        let mut result = Vec::new();
        for tag in tags {
            let tag = tag.trim();
            if tag.is_empty() || tag.chars().count() > 128 || tag.chars().any(char::is_control) {
                return Err(invalid("标签不能为空、超过 128 字或包含控制字符"));
            }
            if seen.insert(tag.to_lowercase()) {
                result.push(tag.to_owned());
            }
        }
        Ok(result)
    };
    Ok(match action {
        BulkAction::AddTags { tags } => BulkAction::AddTags { tags: clean(tags)? },
        BulkAction::RemoveTags { tags } => BulkAction::RemoveTags { tags: clean(tags)? },
        other => other,
    })
}

fn state(conn: &Connection, id: &str, action: &BulkAction) -> AppResult<FieldState> {
    if !conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM works WHERE id=?1)",
        [id],
        |r| r.get::<_, bool>(0),
    )? {
        return Err(invalid("作品已不存在"));
    }
    let mut result = FieldState {
        row: None,
        tags: vec![],
        flag: false,
    };
    match action {
        BulkAction::Collected { .. } | BulkAction::Favorite { .. } => {
            let table = if matches!(action, BulkAction::Collected { .. }) {
                "collected_works"
            } else {
                "favorites"
            };
            result.row = conn
                .query_row(
                    &format!("SELECT created_at FROM {table} WHERE work_id=?1"),
                    [id],
                    |r| Ok(vec![r.get(0)?]),
                )
                .optional()?;
            result.flag = result.row.is_some();
        }
        BulkAction::Listened { .. } => {
            result.row = conn
                .query_row(
                    "SELECT listened,updated_at FROM work_listening_marks WHERE work_id=?1",
                    [id],
                    |r| Ok(vec![r.get::<_, i64>(0)?.to_string(), r.get(1)?]),
                )
                .optional()?;
            result.flag = match &result.row { Some(row) => row[0] == "1", None => conn.query_row("SELECT EXISTS(SELECT 1 FROM work_playback_state WHERE work_id=?1) OR EXISTS(SELECT 1 FROM tracks t JOIN track_versions v ON v.track_id=t.id JOIN playback_positions p ON p.version_id=v.id WHERE t.work_id=?1 AND (p.position_ms>0 OR p.completed=1))", [id], |r| r.get(0))? };
        }
        BulkAction::AddTags { .. } | BulkAction::RemoveTags { .. } => {
            result.row = conn.query_row("SELECT override_mode,value_text,updated_at FROM work_overrides WHERE work_id=?1 AND field='tags'", [id], |r| Ok(vec![r.get(0)?, r.get::<_, Option<String>>(1)?.unwrap_or_default(), r.get(2)?])).optional()?;
            let base: String =
                conn.query_row("SELECT tags_json FROM works WHERE id=?1", [id], |r| {
                    r.get(0)
                })?;
            let raw: Option<String> = conn.query_row("SELECT raw_metadata_json FROM work_sources WHERE work_id=?1 ORDER BY refreshed_at DESC LIMIT 1", [id], |r| r.get(0)).optional()?;
            let meta = raw.and_then(|v| serde_json::from_str(&v).ok());
            let mut overrides = BTreeMap::new();
            if let Some(row) = &result.row {
                overrides.insert(
                    "tags".into(),
                    (
                        row[0].clone(),
                        if row[1].is_empty() {
                            None
                        } else {
                            Some(row[1].clone())
                        },
                    ),
                );
            }
            result.tags = Database::resolve_effective_work_fields_from_data(
                Some(&overrides),
                meta.as_ref(),
                "",
                "",
                "",
                &None,
                "[]",
                &base,
            )
            .5;
        }
    }
    Ok(result)
}
fn tags_after(current: &[String], action: &BulkAction) -> Vec<String> {
    let mut result = current.to_vec();
    match action {
        BulkAction::AddTags { tags } => {
            for tag in tags {
                if !result
                    .iter()
                    .any(|t| t.to_lowercase() == tag.to_lowercase())
                {
                    result.push(tag.clone());
                }
            }
        }
        BulkAction::RemoveTags { tags } => result.retain(|t| {
            !tags
                .iter()
                .any(|tag| t.to_lowercase() == tag.to_lowercase())
        }),
        _ => {}
    }
    result
}
fn changes(before: &FieldState, action: &BulkAction) -> bool {
    match action {
        BulkAction::Collected { value }
        | BulkAction::Favorite { value } => before.flag != *value,
        BulkAction::Listened { value } => before.row.as_ref().is_none_or(|row| (row[0] == "1") != *value),
        _ => tags_after(&before.tags, action) != before.tags,
    }
}
fn eligible(conn: &Connection, id: &str, action: &BulkAction) -> AppResult<()> {
    if matches!(action, BulkAction::Collected { value: false }) && conn.query_row("SELECT EXISTS(SELECT 1 FROM tracks t JOIN track_versions v ON v.track_id=t.id JOIN media_sources m ON m.version_id=v.id WHERE t.work_id=?1 AND m.source_kind='local_imported')", [id], |r| r.get::<_, bool>(0))? {
        return Err(invalid("本地导入作品不支持移出收藏；移除作品索引请使用独立入口"));
    }
    Ok(())
}
fn write(conn: &Connection, id: &str, action: &BulkAction, before: &FieldState) -> AppResult<()> {
    let now = Utc::now().to_rfc3339();
    match action {
        BulkAction::Collected { value } | BulkAction::Favorite { value } => {
            let table = if matches!(action, BulkAction::Collected { .. }) {
                "collected_works"
            } else {
                "favorites"
            };
            if *value {
                conn.execute(
                    &format!("INSERT INTO {table}(work_id,created_at) VALUES (?1,?2)"),
                    params![id, now],
                )?;
            } else {
                conn.execute(&format!("DELETE FROM {table} WHERE work_id=?1"), [id])?;
            }
        }
        BulkAction::Listened { value } => {
            conn.execute("INSERT INTO work_listening_marks(work_id,listened,updated_at) VALUES (?1,?2,?3) ON CONFLICT(work_id) DO UPDATE SET listened=excluded.listened,updated_at=excluded.updated_at", params![id, value, now])?;
        }
        _ => {
            conn.execute("INSERT INTO work_overrides(work_id,field,override_mode,value_text,updated_at) VALUES (?1,'tags','set',?2,?3) ON CONFLICT(work_id,field) DO UPDATE SET override_mode=excluded.override_mode,value_text=excluded.value_text,updated_at=excluded.updated_at", params![id, json(&tags_after(&before.tags, action))?, now])?;
        }
    }
    Ok(())
}
fn restore(conn: &Connection, id: &str, action: &BulkAction, before: &FieldState) -> AppResult<()> {
    if matches!(action, BulkAction::Collected { .. }) && before.row.is_none() {
        eligible(conn, id, &BulkAction::Collected { value: false })?;
    }
    match action {
        BulkAction::Collected { .. } | BulkAction::Favorite { .. } => {
            let table = if matches!(action, BulkAction::Collected { .. }) {
                "collected_works"
            } else {
                "favorites"
            };
            conn.execute(&format!("DELETE FROM {table} WHERE work_id=?1"), [id])?;
            if let Some(row) = &before.row {
                conn.execute(
                    &format!("INSERT INTO {table}(work_id,created_at) VALUES (?1,?2)"),
                    params![id, row[0]],
                )?;
            }
        }
        BulkAction::Listened { .. } => {
            conn.execute("DELETE FROM work_listening_marks WHERE work_id=?1", [id])?;
            if let Some(row) = &before.row {
                conn.execute("INSERT INTO work_listening_marks(work_id,listened,updated_at) VALUES (?1,?2,?3)", params![id,row[0],row[1]])?;
            }
        }
        _ => {
            conn.execute(
                "DELETE FROM work_overrides WHERE work_id=?1 AND field='tags'",
                [id],
            )?;
            if let Some(row) = &before.row {
                conn.execute("INSERT INTO work_overrides(work_id,field,override_mode,value_text,updated_at) VALUES (?1,'tags',?2,?3,?4)", params![id,row[0],if row[1].is_empty() { None } else { Some(&row[1]) },row[2]])?;
            }
        }
    }
    Ok(())
}

impl Database {
    pub fn preview_library_batch(
        &self,
        ids: Vec<String>,
        action: BulkAction,
    ) -> AppResult<BulkReport> {
        if ids.is_empty() || ids.len() > 500 {
            return Err(invalid("每批请选择 1–500 部作品"));
        }
        let action = normalize(action)?;
        self.with_conn(|conn| {
            let tx = conn.transaction()?;
            let mut seen = HashSet::new(); let mut entries = Vec::new();
            for id in ids.into_iter().filter(|id| seen.insert(id.clone())) {
                let title = tx.query_row("SELECT title FROM works WHERE id=?1", [&id], |r| r.get::<_, String>(0)).optional()?.unwrap_or_else(|| "已移除作品".into());
                let before = state(&tx, &id, &action);
                let (status, error) = match &before {
                    Ok(value) => match eligible(&tx, &id, &action) { Ok(()) => (if changes(value, &action) { "ready" } else { "unchanged" }, None), Err(e) => ("failed", Some(e.message)) },
                    Err(e) => ("failed", Some(e.message.clone())),
                };
                entries.push(Entry { item: BulkItem { work_id: id, title, status: status.into(), error }, before: before.ok(), after: None });
            }
            let journal = Journal { id: Uuid::new_v4().to_string(), action, phase: "preview".into(), entries };
            // Bound unexecuted previews; never remove the latest undo record.
            tx.execute("DELETE FROM library_batches WHERE execution_seq IS NULL AND id NOT IN (SELECT id FROM library_batches WHERE execution_seq IS NULL ORDER BY created_at DESC LIMIT 9)", [])?;
            tx.execute("INSERT INTO library_batches(id,journal_json,created_at) VALUES (?1,?2,?3)", params![journal.id,json(&journal)?,Utc::now().to_rfc3339()])?;
            tx.commit()?;
            Ok(journal.report())
        })
    }

    pub fn latest_library_batch(&self) -> AppResult<Option<BulkReport>> {
        self.with_conn(|conn| {
            latest_id(conn)?
                .map(|id| load(conn, &id).map(|j| j.report()))
                .transpose()
        })
    }

    pub fn execute_library_batch(
        &self,
        id: &str,
        retry_ids: Option<Vec<String>>,
    ) -> AppResult<BulkReport> {
        self.with_conn(|conn| {
            let tx = conn.transaction()?;
            let mut journal = load(&tx, id)?;
            let retry = retry_ids.is_some();
            if !retry && journal.phase != "preview" { return Ok(journal.report()); }
            if retry && journal.phase != "applied" { return Err(invalid("仅已执行批次的失败项可以重试")); }
            let has_seq: bool = tx.query_row("SELECT execution_seq IS NOT NULL FROM library_batches WHERE id=?1", [id], |r| r.get(0))?;
            if retry && has_seq && latest_id(&tx)?.as_deref() != Some(id) { return Err(invalid("已有更新的批量修改，请重新预览失败项")); }
            let retry_ids: HashSet<String> = retry_ids.unwrap_or_default().into_iter().collect();
            if retry && (retry_ids.is_empty() || retry_ids.iter().any(|id| !journal.entries.iter().any(|e| &e.item.work_id == id && e.item.status == "failed"))) { return Err(invalid("只能选择此批次的失败项重试")); }
            let mut applied = false;
            for entry in &mut journal.entries {
                if (!retry && entry.item.status != "ready") || (retry && !retry_ids.contains(&entry.item.work_id)) { continue; }
                tx.execute_batch("SAVEPOINT bulk_item")?;
                let outcome = (|| -> AppResult<FieldState> {
                    let before = entry.before.as_ref().ok_or_else(|| invalid("作品未通过原预览，请重新预览"))?;
                    eligible(&tx, &entry.item.work_id, &journal.action)?;
                    if state(&tx, &entry.item.work_id, &journal.action)? != *before { return Err(invalid("字段已在预览后变化，请重新预览")); }
                    write(&tx, &entry.item.work_id, &journal.action, before)?;
                    state(&tx, &entry.item.work_id, &journal.action)
                })();
                match outcome {
                    Ok(after) => { tx.execute_batch("RELEASE bulk_item")?; entry.after = Some(after); entry.item.status = "applied".into(); entry.item.error = None; applied = true; }
                    Err(e) => { tx.execute_batch("ROLLBACK TO bulk_item; RELEASE bulk_item")?; entry.item.status = "failed".into(); entry.item.error = Some(e.message); }
                }
            }
            if applied && !has_seq { tx.execute("UPDATE library_batches SET execution_seq=(SELECT COALESCE(MAX(execution_seq),0)+1 FROM library_batches) WHERE id=?1", [id])?; }
            journal.phase = "applied".into(); store(&tx, &journal)?;
            tx.commit()?;
            Ok(journal.report())
        })
    }

    pub fn undo_library_batch(&self, id: &str) -> AppResult<BulkReport> {
        self.with_conn(|conn| {
            let tx = conn.transaction()?;
            if latest_id(&tx)?.as_deref() != Some(id) {
                return Err(invalid("只能撤销最近一次批量修改"));
            }
            let mut journal = load(&tx, id)?;
            for entry in &mut journal.entries {
                if entry.item.status != "applied" && entry.item.status != "undoFailed" {
                    continue;
                }
                tx.execute_batch("SAVEPOINT bulk_undo")?;
                let outcome = (|| -> AppResult<()> {
                    let after = entry
                        .after
                        .as_ref()
                        .ok_or_else(|| invalid("缺少撤销记录"))?;
                    if state(&tx, &entry.item.work_id, &journal.action)? != *after {
                        return Err(invalid("该字段随后发生变化，已保留当前状态"));
                    }
                    restore(
                        &tx,
                        &entry.item.work_id,
                        &journal.action,
                        entry
                            .before
                            .as_ref()
                            .ok_or_else(|| invalid("缺少原始记录"))?,
                    )
                })();
                match outcome {
                    Ok(()) => {
                        tx.execute_batch("RELEASE bulk_undo")?;
                        entry.item.status = "undone".into();
                        entry.item.error = None;
                    }
                    Err(e) => {
                        tx.execute_batch("ROLLBACK TO bulk_undo; RELEASE bulk_undo")?;
                        entry.item.status = "undoFailed".into();
                        entry.item.error = Some(e.message);
                    }
                }
            }
            journal.phase = if journal
                .entries
                .iter()
                .any(|e| e.item.status == "undoFailed")
            {
                "undoPartial"
            } else {
                "undone"
            }
            .into();
            store(&tx, &journal)?;
            tx.commit()?;
            Ok(journal.report())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn seed(db: &Database, id: &str) {
        db.with_conn(|conn| {
            conn.execute("INSERT INTO works(id,title,tags_json,created_at,updated_at) VALUES (?1,?1,'[\"睡眠\"]','now','now')", [id])?;
            conn.execute("INSERT INTO tracks(id,work_id,title,sort_key,created_at) VALUES (?1,?1,'音轨','1','now')", [id])?;
            conn.execute("INSERT INTO track_versions(id,track_id,created_at) VALUES (?1,?1,'now')", [id])?;
            Ok(())
        }).unwrap();
    }
    fn sql(db: &Database, value: &str) {
        db.with_conn(|conn| {
            conn.execute_batch(value)?;
            Ok(())
        })
        .unwrap();
    }
    fn read(db: &Database, id: &str, action: &BulkAction) -> FieldState {
        db.with_conn(|conn| state(conn, id, action)).unwrap()
    }
    fn preview(db: &Database, ids: &[&str], action: BulkAction) -> BulkReport {
        db.preview_library_batch(ids.iter().map(|s| s.to_string()).collect(), action)
            .unwrap()
    }

    #[test]
    fn partial_failure_retry_idempotency_and_restart_undo_t1502() {
        let dir = tempdir().unwrap();
        let db = Database::open_and_migrate(&dir.path().join("data"), &dir.path().join("cache"))
            .unwrap();
        seed(&db, "a");
        seed(&db, "b");
        let action = BulkAction::Favorite { value: true };
        let batch = preview(&db, &["a", "b", "a"], action.clone());
        assert_eq!(batch.items.len(), 2);
        assert!(!read(&db, "a", &action).flag);
        sql(&db, "CREATE TRIGGER fail_b BEFORE INSERT ON favorites WHEN NEW.work_id='b' BEGIN SELECT RAISE(ABORT,'test failure'); END;");
        let result = db.execute_library_batch(&batch.id, None).unwrap();
        assert_eq!(result.items[0].status, "applied");
        assert_eq!(result.items[1].status, "failed");
        assert!(!read(&db, "b", &action).flag);
        let original_a = read(&db, "a", &action);
        assert_eq!(
            db.execute_library_batch(&batch.id, None).unwrap().items[1].status,
            "failed"
        );
        sql(&db, "DROP TRIGGER fail_b;");
        assert!(db
            .execute_library_batch(&batch.id, Some(vec!["a".into()]))
            .is_err());
        assert!(db
            .execute_library_batch(&batch.id, Some(vec!["b".into()]))
            .unwrap()
            .items
            .iter()
            .all(|i| i.status == "applied"));
        assert_eq!(read(&db, "a", &action), original_a);
        drop(db);
        let db = Database::open_and_migrate(&dir.path().join("data"), &dir.path().join("cache"))
            .unwrap();
        assert_eq!(db.latest_library_batch().unwrap().unwrap().id, batch.id);
        assert!(db
            .undo_library_batch(&batch.id)
            .unwrap()
            .items
            .iter()
            .all(|i| i.status == "undone"));
        assert!(!read(&db, "a", &action).flag && !read(&db, "b", &action).flag);
        assert_eq!(db.undo_library_batch(&batch.id).unwrap().phase, "undone");
        assert_eq!(
            db.execute_library_batch(&batch.id, None).unwrap().phase,
            "undone"
        );
    }

    #[test]
    fn stale_previews_and_undo_conflicts_preserve_newer_fields_t1502() {
        let dir = tempdir().unwrap();
        let db = Database::open_and_migrate(&dir.path().join("data"), &dir.path().join("cache"))
            .unwrap();
        seed(&db, "a");
        seed(&db, "b");
        let action = BulkAction::Favorite { value: true };
        let batch = preview(&db, &["a", "b"], action.clone());
        db.toggle_favorite("a").unwrap();
        let result = db.execute_library_batch(&batch.id, None).unwrap();
        assert_eq!(result.items[0].status, "failed");
        assert_eq!(result.items[1].status, "applied");
        sql(
            &db,
            "UPDATE favorites SET created_at='newer-edit' WHERE work_id='b';",
        );
        assert_eq!(
            db.undo_library_batch(&batch.id).unwrap().items[1].status,
            "undoFailed"
        );
        assert_eq!(read(&db, "b", &action).row.unwrap(), vec!["newer-edit"]);
        let newer = preview(&db, &["a"], BulkAction::Collected { value: true });
        db.execute_library_batch(&newer.id, None).unwrap();
        assert!(db.undo_library_batch(&batch.id).is_err());
        db.undo_library_batch(&newer.id).unwrap();
        assert!(read(&db, "a", &action).flag);
    }

    #[test]
    fn tags_restore_override_absence_and_cleared_mode_t1502() {
        let dir = tempdir().unwrap();
        let db = Database::open_and_migrate(&dir.path().join("data"), &dir.path().join("cache"))
            .unwrap();
        seed(&db, "a");
        seed(&db, "b");
        db.set_work_override("b", "tags", "cleared", None).unwrap();
        let action = BulkAction::AddTags {
            tags: vec![" ASMR ".into(), "asmr".into()],
        };
        let before_a = read(&db, "a", &action);
        let before_b = read(&db, "b", &action);
        let batch = preview(&db, &["a", "b"], action.clone());
        db.execute_library_batch(&batch.id, None).unwrap();
        assert_eq!(read(&db, "a", &action).tags, vec!["睡眠", "ASMR"]);
        assert_eq!(read(&db, "b", &action).tags, vec!["ASMR"]);
        db.set_work_override("a", "title", "set", Some("新标题"))
            .unwrap();
        db.undo_library_batch(&batch.id).unwrap();
        assert_eq!(read(&db, "a", &action), before_a);
        assert_eq!(read(&db, "b", &action), before_b);
        let removal = preview(
            &db,
            &["a"],
            BulkAction::RemoveTags {
                tags: vec!["睡眠".into()],
            },
        );
        db.execute_library_batch(&removal.id, None).unwrap();
        assert!(read(&db, "a", &action).tags.is_empty());
        db.undo_library_batch(&removal.id).unwrap();
        assert_eq!(read(&db, "a", &action), before_a);
    }

    #[test]
    fn listening_marks_keep_progress_and_restore_automatic_state_t1502() {
        let dir = tempdir().unwrap();
        let db = Database::open_and_migrate(&dir.path().join("data"), &dir.path().join("cache"))
            .unwrap();
        seed(&db, "a");
        db.save_playback_position("a", 4500, false, true).unwrap();
        let action = BulkAction::Listened { value: false };
        let batch = preview(&db, &["a"], action.clone());
        db.execute_library_batch(&batch.id, None).unwrap();
        assert!(!db.list_works("recent", None).unwrap()[0].is_listened);
        assert_eq!(db.get_playback_position("a").unwrap(), (4500, false));
        db.save_playback_position("a", 9000, true, true).unwrap();
        assert!(!read(&db, "a", &action).flag);
        db.undo_library_batch(&batch.id).unwrap();
        assert!(db.list_works("recent", None).unwrap()[0].is_listened);
        assert_eq!(db.get_playback_position("a").unwrap(), (9000, true));
        assert!(read(&db, "a", &action).row.is_none());
    }

    #[test]
    fn collection_removal_retains_media_and_rejects_imported_sources_t1502() {
        let dir = tempdir().unwrap();
        let db = Database::open_and_migrate(&dir.path().join("data"), &dir.path().join("cache"))
            .unwrap();
        let file = dir.path().join("synthetic.bin");
        std::fs::write(&file, b"synthetic").unwrap();
        seed(&db, "a");
        seed(&db, "b");
        db.with_conn(|conn| {
            for (id, kind) in [("a", "local_downloaded"), ("b", "local_imported")] {
                conn.execute("INSERT INTO media_sources(id,version_id,source_kind,absolute_path,format,created_at,updated_at) VALUES (?1,?1,?2,?3,'wav','now','now')", params![id,kind,file.to_string_lossy()])?;
                conn.execute("INSERT INTO collected_works(work_id,created_at) VALUES (?1,'original')", [id])?;
            }
            Ok(())
        }).unwrap();
        let action = BulkAction::Collected { value: false };
        let batch = preview(&db, &["a", "b", "missing"], action.clone());
        assert_eq!(
            batch
                .items
                .iter()
                .map(|i| i.status.as_str())
                .collect::<Vec<_>>(),
            vec!["ready", "failed", "failed"]
        );
        db.execute_library_batch(&batch.id, None).unwrap();
        assert!(!read(&db, "a", &action).flag);
        assert!(read(&db, "b", &action).flag);
        assert_eq!(std::fs::read(&file).unwrap(), b"synthetic");
        db.undo_library_batch(&batch.id).unwrap();
        assert_eq!(read(&db, "a", &action).row.unwrap(), vec!["original"]);
    }

    #[test]
    fn journal_failure_rolls_back_data_and_noops_keep_undo_slot_t1502() {
        let dir = tempdir().unwrap();
        let db = Database::open_and_migrate(&dir.path().join("data"), &dir.path().join("cache"))
            .unwrap();
        seed(&db, "a");
        let action = BulkAction::Favorite { value: true };
        let batch = preview(&db, &["a"], action.clone());
        sql(&db, "CREATE TRIGGER fail_journal BEFORE UPDATE OF journal_json ON library_batches BEGIN SELECT RAISE(ABORT,'test journal failure'); END;");
        assert!(db.execute_library_batch(&batch.id, None).is_err());
        assert!(!read(&db, "a", &action).flag);
        assert!(db.latest_library_batch().unwrap().is_none());
        sql(&db, "DROP TRIGGER fail_journal;");
        db.execute_library_batch(&batch.id, None).unwrap();
        let noop = preview(&db, &["a", "missing"], action.clone());
        assert_eq!(noop.items[0].status, "unchanged");
        db.execute_library_batch(&noop.id, None).unwrap();
        assert_eq!(db.latest_library_batch().unwrap().unwrap().id, batch.id);
        assert!(db.preview_library_batch(vec![], action.clone()).is_err());
        assert!(db
            .preview_library_batch(vec!["a".into(); 501], action)
            .is_err());
        assert!(db
            .preview_library_batch(
                vec!["a".into()],
                BulkAction::AddTags {
                    tags: vec!["\0".into()]
                }
            )
            .is_err());
        assert!(serde_json::from_str::<BulkAction>(r#"{"kind":"deleteFiles"}"#).is_err());
    }
}
