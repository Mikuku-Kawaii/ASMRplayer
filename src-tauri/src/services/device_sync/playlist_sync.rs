use super::{catalog::hash, protocol::Values};
use crate::{
    db::playlists::{self, Playlist},
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
pub fn referenced(values: &Values) -> BTreeSet<String> {
    values
        .iter()
        .filter(|(k, v)| k.starts_with("playlist|") && !v.is_null())
        .flat_map(|(_, v)| {
            v["items"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(str::to_owned)
        })
        .collect()
}
pub fn snapshot(c: &Connection, versions: &BTreeMap<String, String>) -> AppResult<Values> {
    Ok(playlists::read_all(c)?
        .into_iter()
        .map(|p| {
            let mut seen = BTreeSet::new();
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
) -> AppResult<()> {
    let mut lookup = BTreeMap::new();
    let mut stmt=c.prepare("SELECT v.id FROM track_versions v ORDER BY EXISTS(SELECT 1 FROM media_sources s WHERE s.version_id=v.id AND s.availability='available') DESC,v.id")?;
    for row in stmt.query_map([], |r| r.get::<_, String>(0))? {
        let id = row?;
        lookup.entry(key(&id, versions)).or_insert(id.clone());
        lookup.insert(fallback(&id), id);
    }
    for (record, value) in values.iter().filter(|(k, _)| k.starts_with("playlist|")) {
        let id = record.split('|').nth(2).unwrap_or("");
        if value.is_null() {
            c.execute("DELETE FROM playlists WHERE id=?1", [id])?;
            continue;
        }
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
                sort_order: value["sortOrder"].as_i64().unwrap_or(0),
            },
        )?;
    }
    Ok(())
}
