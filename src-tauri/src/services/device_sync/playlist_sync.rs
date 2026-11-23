use super::{catalog::hash, protocol::Values};
use crate::{
    db::playlists::{self, Playlist, WorkItem},
    domain::AppResult,
};
use rusqlite::Connection;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

fn fallback(id: &str) -> String {
    hash(format!("playlist-pc|{id}").as_bytes())
}
fn key(id: &str, versions: &BTreeMap<String, String>) -> String {
    versions.get(id).cloned().unwrap_or_else(|| {
        id.strip_prefix("sync:")
            .map(str::to_owned)
            .unwrap_or_else(|| fallback(id))
    })
}
pub fn valid(identity: &str, id: &str, v: &Value) -> bool {
    if id.is_empty() || hash(id.as_bytes()) != identity {
        return false;
    }
    if v.is_null() {
        return true;
    }
    let Some(items) = v["items"].as_array() else {
        return false;
    };
    v["name"]
        .as_str()
        .is_some_and(|s| !s.trim().is_empty() && s.chars().count() <= 80)
        && v["sortOrder"]
            .as_u64()
            .is_some_and(|v| v <= 1_000_000_000_000)
        && items.len() <= 500
        && items.iter().all(|v| {
            v.as_str().is_some_and(|s| {
                s.len() == 64
                    && s.bytes()
                        .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
            })
        })
        && items
            .iter()
            .filter_map(Value::as_str)
            .collect::<BTreeSet<_>>()
            .len()
            == items.len()
}
pub fn valid_works(identity: &str, id: &str, v: &Value) -> bool {
    if id.is_empty() || hash(id.as_bytes()) != identity {
        return false;
    }
    if v.is_null() {
        return true;
    }
    let Some(items) = v["items"].as_array() else {
        return false;
    };
    let mut seen = BTreeSet::new();
    v["name"]
        .as_str()
        .is_some_and(|s| playlists::valid_name(id, s))
        && v["sortOrder"]
            .as_u64()
            .is_some_and(|n| n <= 1_000_000_000_000)
        && items.iter().all(|raw| {
            serde_json::from_value::<WorkItem>(raw.clone()).is_ok_and(|w| {
                w.valid()
                    && w.id.len() == 64
                    && w.id
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                    && seen.insert(w.id)
            })
        })
}
pub fn referenced(values: &Values) -> BTreeSet<String> {
    values
        .iter()
        .filter(|(k, v)| (k.starts_with("playlist|") || k.starts_with("worklist|")) && !v.is_null())
        .flat_map(|(_, v)| {
            v["items"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|item| item.as_str().or_else(|| item["id"].as_str()))
                .map(str::to_owned)
        })
        .collect()
}
pub fn snapshot(
    c: &Connection,
    versions: &BTreeMap<String, String>,
    works: &BTreeMap<String, String>,
) -> AppResult<Values> {
    Ok(playlists::read_all(c)?
        .into_iter()
        .map(|p| {
            let mut seen = BTreeSet::new();
            if let Some(mut items) = p.work_items {
                for w in &mut items {
                    w.id = key(&w.id, works);
                }
                items.retain(|w| seen.insert(w.id.clone()));
                return (
                    format!("worklist|{}|{}", hash(p.id.as_bytes()), p.id),
                    json!({"name":p.name,"items":items,"sortOrder":p.sort_order}),
                );
            }
            let items = p
                .version_ids
                .iter()
                .map(|id| key(id, versions))
                .filter(|key| seen.insert(key.clone()))
                .collect::<Vec<_>>();
            (
                format!("playlist|{}|{}", hash(p.id.as_bytes()), p.id),
                json!({"name":p.name,"items":items,"sortOrder":p.sort_order}),
            )
        })
        .collect())
}
pub fn apply(
    c: &Connection,
    values: &Values,
    versions: &BTreeMap<String, String>,
    works: &BTreeMap<String, String>,
) -> AppResult<()> {
    let mut lookup = BTreeMap::new();
    let mut stmt=c.prepare("SELECT v.id FROM track_versions v ORDER BY EXISTS(SELECT 1 FROM media_sources s WHERE s.version_id=v.id AND s.availability='available') DESC,v.id")?;
    for row in stmt.query_map([], |r| r.get::<_, String>(0))? {
        let id = row?;
        lookup.entry(key(&id, versions)).or_insert(id.clone());
        lookup.insert(fallback(&id), id);
    }
    let mut work_lookup = BTreeMap::new();
    let mut rows = c.prepare("SELECT id FROM works ORDER BY EXISTS(SELECT 1 FROM collected_works WHERE work_id=works.id) DESC,id")?;
    for row in rows.query_map([], |r| r.get::<_, String>(0))? {
        let id = row?;
        work_lookup.entry(key(&id, works)).or_insert(id.clone());
        work_lookup.insert(fallback(&id), id);
    }
    for (record, value) in values
        .iter()
        .filter(|(k, _)| k.starts_with("playlist|") || k.starts_with("worklist|"))
    {
        let is_works = record.starts_with("worklist|");
        let id = record.split('|').nth(2).unwrap_or("");
        if value.is_null() {
            c.execute(
                "DELETE FROM playlists WHERE id=?1 AND (work_items IS NOT NULL)=?2",
                rusqlite::params![id, is_works],
            )?;
            continue;
        }
        let work_items = if is_works {
            let mut items: Vec<WorkItem> = serde_json::from_value(value["items"].clone())
                .map_err(|_| super::fail("作品集内容无效"))?;
            for w in &mut items {
                w.id = work_lookup
                    .get(&w.id)
                    .cloned()
                    .unwrap_or_else(|| format!("sync:{}", w.id));
            }
            Some(items)
        } else {
            None
        };
        let version_ids = value["items"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(|key| {
                lookup
                    .get(key)
                    .cloned()
                    .unwrap_or_else(|| format!("sync:{key}"))
            })
            .collect();
        playlists::put(
            c,
            &Playlist {
                id: id.into(),
                name: value["name"].as_str().unwrap_or("").into(),
                version_ids,
                work_items,
                sort_order: value["sortOrder"].as_i64().unwrap_or(0),
            },
        )?;
    }
    Ok(())
}
