use super::Database;
use crate::domain::{AppError, AppResult, ErrorCode, WorkSummary};
use chrono::Utc;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, path::Path};
use uuid::Uuid;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum ListeningFilter {
    #[default]
    Any,
    Unplayed,
    InProgress,
    Played,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct CollectionFilters {
    pub keyword: String,
    pub voice_actors: Vec<String>,
    pub tags: Vec<String>,
    pub collected: Option<bool>,
    pub favorite: Option<bool>,
    pub listening: ListeningFilter,
    pub offline: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SmartCollection {
    pub id: String,
    pub name: String,
    pub filters: CollectionFilters,
    pub updated_at: String,
}

fn invalid(message: &str) -> AppError {
    AppError::new(ErrorCode::InvalidInput, message)
}

fn clean_text(value: &str, max: usize) -> AppResult<String> {
    let value = value.trim();
    if value.chars().count() > max || value.chars().any(char::is_control) {
        return Err(invalid("集合名称或筛选条件过长，或含有不支持的控制字符"));
    }
    Ok(value.to_owned())
}

impl CollectionFilters {
    fn normalized(&self) -> AppResult<Self> {
        fn terms(values: &[String]) -> AppResult<Vec<String>> {
            if values.len() > 16 {
                return Err(invalid("声优和标签各最多支持 16 个条件"));
            }
            let mut unique = HashSet::new();
            let mut result = Vec::new();
            for value in values {
                let value = clean_text(value, 128)?;
                if !value.is_empty() && unique.insert(value.to_lowercase()) {
                    result.push(value);
                }
            }
            Ok(result)
        }
        Ok(Self {
            keyword: clean_text(&self.keyword, 200)?,
            voice_actors: terms(&self.voice_actors)?,
            tags: terms(&self.tags)?,
            ..self.clone()
        })
    }
}

impl Database {
    pub fn list_smart_collections(&self) -> AppResult<Vec<SmartCollection>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare("SELECT id, name, filters_json, updated_at FROM smart_collections ORDER BY name_key, id")?;
            let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?)))?;
            let mut collections = Vec::new();
            for row in rows {
                let (id, name, json, updated_at) = row?;
                let filters: CollectionFilters = serde_json::from_str(&json).map_err(|_| invalid("集合条件无法读取，请检查备份或更新应用"))?;
                collections.push(SmartCollection { id, name, filters: filters.normalized()?, updated_at });
            }
            Ok(collections)
        })
    }

    pub fn save_smart_collection(
        &self,
        id: Option<&str>,
        name: &str,
        filters: &CollectionFilters,
    ) -> AppResult<SmartCollection> {
        let name = clean_text(name, 64)?;
        if name.is_empty() {
            return Err(invalid("请填写集合名称"));
        }
        let filters = filters.normalized()?;
        let json = serde_json::to_string(&filters).map_err(|_| invalid("无法保存集合条件"))?;
        let key = name.to_lowercase();
        let now = Utc::now().to_rfc3339();
        let collection_id = id
            .map(str::to_owned)
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        self.with_conn(|conn| {
            // Check and write under the same database lock; updates never create missing IDs.
            let duplicate: Option<String> = conn.query_row("SELECT id FROM smart_collections WHERE name_key=?1 AND id<>?2", params![key, collection_id], |r| r.get(0)).optional()?;
            if duplicate.is_some() { return Err(invalid("已有同名集合，请换一个名称")); }
            if id.is_some() {
                if conn.execute("UPDATE smart_collections SET name=?1, name_key=?2, filters_json=?3, updated_at=?4 WHERE id=?5", params![name, key, json, now, collection_id])? == 0 {
                    return Err(invalid("集合已被移除，请另存为新集合"));
                }
            } else {
                conn.execute("INSERT INTO smart_collections (id,name,name_key,filters_json,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,?5)", params![collection_id, name, key, json, now])?;
            }
            Ok(SmartCollection { id: collection_id, name, filters, updated_at: now })
        })
    }

    pub fn remove_smart_collection(&self, id: &str) -> AppResult<()> {
        self.with_conn(|conn| {
            conn.execute("DELETE FROM smart_collections WHERE id=?1", params![id])?;
            Ok(())
        })
    }

    // T1501: rules run against all known works, before the UI's display pagination.
    // Reuse effective metadata overrides, keyword matching and global NSFW policy.
    pub fn query_smart_collection(
        &self,
        filters: &CollectionFilters,
    ) -> AppResult<Vec<WorkSummary>> {
        let filters = filters.normalized()?;
        let mut works = self.list_works("collection", Some(&filters.keyword))?;
        let local_paths = self.with_conn(|conn| {
            let mut local_paths = Vec::<(String, String)>::new();
            if filters.offline.is_some() {
                let mut stmt = conn.prepare("SELECT DISTINCT t.work_id, m.absolute_path FROM tracks t JOIN track_versions v ON v.track_id=t.id JOIN media_sources m ON m.version_id=v.id WHERE m.source_kind IN ('local_imported','local_downloaded') AND m.availability='available' AND m.absolute_path IS NOT NULL")?;
                for row in stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
                    local_paths.push(row?);
                }
            }
            Ok(local_paths)
        })?;
        // Disk checks run on the blocking worker, outside the database mutex.
        // A cached 'available' flag alone must not count an unplugged disk.
        let mut offline = HashSet::<String>::new();
        for (id, path) in local_paths {
            if !offline.contains(&id) && Path::new(&path).is_file() {
                offline.insert(id);
            }
        }
        let contains_all = |actual: &[String], expected: &[String]| {
            expected.iter().all(|term| {
                actual
                    .iter()
                    .any(|v| v.trim().to_lowercase() == term.to_lowercase())
            })
        };
        works.retain(|work| {
            filters
                .collected
                .is_none_or(|value| value == work.is_collected)
                && filters
                    .favorite
                    .is_none_or(|value| value == work.is_favorite)
                && filters
                    .offline
                    .is_none_or(|value| value == offline.contains(&work.id))
                && match filters.listening {
                    ListeningFilter::Any => true,
                    ListeningFilter::Played => work.listening_status == crate::domain::ListeningStatus::Played,
                    ListeningFilter::InProgress => work.listening_status == crate::domain::ListeningStatus::InProgress,
                    ListeningFilter::Unplayed => work.listening_status == crate::domain::ListeningStatus::Unplayed,
                }
                && contains_all(&work.voice_actors, &filters.voice_actors)
                && contains_all(&work.tags, &filters.tags)
        });
        Ok(works)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn seed(db: &Database, id: &str, local: Option<&Path>) {
        db.with_conn(|conn| {
            conn.execute("INSERT INTO works(id,title,voice_actors_json,tags_json,created_at,updated_at) VALUES (?1,?2,'[\"声优 A\",\"声优 B\"]','[\"耳かき\",\"睡眠\"]','now','now')", params![id, format!("作品 {id}")])?;
            conn.execute("INSERT INTO tracks(id,work_id,title,sort_key,created_at) VALUES (?1,?1,'音轨','1','now')", params![id])?;
            conn.execute("INSERT INTO track_versions(id,track_id,created_at) VALUES (?1,?1,'now')", params![id])?;
            if let Some(path) = local {
                conn.execute("INSERT INTO media_sources(id,version_id,source_kind,absolute_path,format,created_at,updated_at) VALUES (?1,?1,'local_imported',?2,'wav','now','now')", params![id, path.to_string_lossy()])?;
            }
            Ok(())
        }).unwrap();
    }

    #[test]
    fn named_rules_survive_restart_validate_updates_and_remove_without_touching_works_t1501() {
        let dir = tempdir().unwrap();
        let db = Database::open_and_migrate(&dir.path().join("data"), &dir.path().join("cache"))
            .unwrap();
        let file = dir.path().join("synthetic.bin");
        std::fs::write(&file, b"synthetic test fixture").unwrap();
        seed(&db, "one", Some(&file));
        let filters = CollectionFilters {
            keyword: " 作品 ".into(),
            voice_actors: vec!["声优 A".into(), "声优 a".into()],
            ..Default::default()
        };
        let saved = db.save_smart_collection(None, " Night ", &filters).unwrap();
        assert_eq!(saved.name, "Night");
        assert_eq!(saved.filters.voice_actors.len(), 1);
        assert_eq!(saved.filters.keyword, "作品");
        assert!(db.save_smart_collection(None, "night", &filters).is_err());
        assert!(db.save_smart_collection(None, "  ", &filters).is_err());
        assert!(db
            .save_smart_collection(None, &"长".repeat(65), &filters)
            .is_err());
        assert!(db
            .save_smart_collection(
                None,
                "invalid",
                &CollectionFilters {
                    tags: vec!["x".into(); 17],
                    ..Default::default()
                }
            )
            .is_err());
        assert!(db
            .query_smart_collection(&CollectionFilters {
                keyword: "\0".into(),
                ..Default::default()
            })
            .is_err());
        assert!(serde_json::from_str::<CollectionFilters>(r#"{"listening":"unknown"}"#).is_err());
        assert!(serde_json::from_str::<CollectionFilters>(r#"{"futureRule":true}"#).is_err());
        let changed = db
            .save_smart_collection(
                Some(&saved.id),
                "改名集合",
                &CollectionFilters {
                    offline: Some(true),
                    ..Default::default()
                },
            )
            .unwrap();
        drop(db);
        let db = Database::open_and_migrate(&dir.path().join("data"), &dir.path().join("cache"))
            .unwrap();
        let restored = db.list_smart_collections().unwrap();
        assert_eq!(restored.len(), 1);
        assert_eq!(restored[0].id, saved.id);
        assert_eq!(restored[0].name, "改名集合");
        assert_eq!(restored[0].filters, changed.filters);
        db.remove_smart_collection(&saved.id).unwrap();
        db.remove_smart_collection(&saved.id).unwrap();
        assert!(db.list_smart_collections().unwrap().is_empty());
        assert!(db
            .save_smart_collection(Some(&saved.id), "不能复活", &filters)
            .is_err());
        assert_eq!(
            db.query_smart_collection(&CollectionFilters::default())
                .unwrap()
                .len(),
            1
        );
        assert!(file.is_file());
    }

    #[test]
    fn rules_follow_current_metadata_membership_progress_and_real_offline_files_t1501() {
        let dir = tempdir().unwrap();
        let db = Database::open_and_migrate(&dir.path().join("data"), &dir.path().join("cache"))
            .unwrap();
        let file = dir.path().join("synthetic.bin");
        std::fs::write(&file, b"synthetic").unwrap();
        seed(&db, "available", Some(&file));
        seed(&db, "missing", Some(&dir.path().join("absent.bin")));
        seed(&db, "online", None);
        // More than a UI page ensures rules don't just filter the first 24 results.
        for i in 0..28 {
            seed(&db, &format!("extra-{i}"), None);
        }
        let filters = CollectionFilters {
            voice_actors: vec!["声优 A".into(), "声优 B".into()],
            tags: vec!["耳かき".into(), "睡眠".into()],
            offline: Some(true),
            listening: ListeningFilter::Unplayed,
            ..Default::default()
        };
        let saved = db
            .save_smart_collection(None, "可用且未听", &filters)
            .unwrap();
        assert_eq!(
            db.query_smart_collection(&saved.filters)
                .unwrap()
                .iter()
                .map(|w| w.id.as_str())
                .collect::<Vec<_>>(),
            vec!["available"]
        );
        assert_eq!(
            db.query_smart_collection(&CollectionFilters {
                collected: Some(false),
                offline: Some(false),
                ..Default::default()
            })
            .unwrap()
            .len(),
            30
        );
        db.toggle_favorite("available").unwrap();
        db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO collected_works(work_id,created_at) VALUES ('available','now')",
                [],
            )?;
            Ok(())
        })
        .unwrap();
        assert_eq!(
            db.query_smart_collection(&CollectionFilters {
                collected: Some(true),
                favorite: Some(true),
                ..Default::default()
            })
            .unwrap()
            .len(),
            1
        );
        db.save_playback_position("available", 1000, false, true)
            .unwrap();
        assert!(db
            .query_smart_collection(&saved.filters)
            .unwrap()
            .is_empty());
        assert_eq!(
            db.query_smart_collection(&CollectionFilters {
                listening: ListeningFilter::InProgress,
                ..Default::default()
            })
            .unwrap()
            .len(),
            1
        );
        db.set_work_override("online", "tags", "set", Some(r#"["独有标签"]"#))
            .unwrap();
        let overridden = db
            .query_smart_collection(&CollectionFilters {
                tags: vec!["独有标签".into()],
                ..Default::default()
            })
            .unwrap();
        assert_eq!(overridden.len(), 1);
        assert_eq!(overridden[0].id, "online");
        assert!(db
            .query_smart_collection(&CollectionFilters {
                tags: vec!["独有标签".into(), "睡眠".into()],
                ..Default::default()
            })
            .unwrap()
            .is_empty());
        db.set_work_override("online", "tags", "set", Some(r#"["R18"]"#))
            .unwrap();
        assert!(db
            .query_smart_collection(&CollectionFilters {
                keyword: "online".into(),
                ..Default::default()
            })
            .unwrap()
            .is_empty());
        db.set_setting("nsfw_enabled", "true").unwrap();
        assert_eq!(
            db.query_smart_collection(&CollectionFilters {
                keyword: "online".into(),
                ..Default::default()
            })
            .unwrap()
            .len(),
            1
        );
        std::fs::remove_file(&file).unwrap();
        assert!(db
            .query_smart_collection(&CollectionFilters {
                offline: Some(true),
                ..Default::default()
            })
            .unwrap()
            .is_empty());
    }
}
