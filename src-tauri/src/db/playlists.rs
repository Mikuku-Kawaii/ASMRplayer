use super::Database;
use crate::domain::{AppError, AppResult, ErrorCode};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Playlist {
    pub id: String,
    pub name: String,
    pub version_ids: Vec<String>,
    pub sort_order: i64,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistTrack {
    pub version_id: String,
    pub title: String,
    pub work_title: String,
    pub cover_ref: Option<String>,
    pub available: bool,
}
fn invalid(text: &str) -> AppError {
    AppError::new(ErrorCode::InvalidInput, text)
}
pub(crate) fn read_all(c: &Connection) -> AppResult<Vec<Playlist>> {
    let mut s =
        c.prepare("SELECT id,name,version_ids,sort_order FROM playlists ORDER BY sort_order,id")?;
    let rows = s
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    rows.into_iter()
        .map(|(id, name, json, sort_order)| {
            Ok(Playlist {
                id,
                name,
                sort_order,
                version_ids: serde_json::from_str(&json)
                    .map_err(|_| invalid("歌单内容无法读取"))?,
            })
        })
        .collect()
}
pub(crate) fn put(c: &Connection, p: &Playlist) -> AppResult<()> {
    let name = p.name.trim();
    if name.is_empty()
        || name.chars().count() > 80
        || p.version_ids.len() > 500
        || p.version_ids
            .iter()
            .any(|id| id.is_empty() || id.len() > 128)
    {
        return Err(invalid("歌单名称为 1–80 个字，每个歌单最多 500 首音轨"));
    }
    c.execute("INSERT INTO playlists(id,name,version_ids,sort_order) VALUES(?1,?2,?3,?4) ON CONFLICT(id) DO UPDATE SET name=excluded.name,version_ids=excluded.version_ids,sort_order=excluded.sort_order",
        params![p.id,name,serde_json::to_string(&p.version_ids).map_err(|_|invalid("歌单无法保存"))?,p.sort_order])?;
    Ok(())
}
impl Database {
    pub fn list_playlists(&self) -> AppResult<Vec<Playlist>> {
        self.with_conn(|c| read_all(c))
    }
    pub fn create_playlist(&self, name: &str, ids: Vec<String>) -> AppResult<Playlist> {
        self.with_conn(|c| {
            let tx = c.transaction()?;
            let mut seen = HashSet::new();
            let ids = ids
                .into_iter()
                .filter(|id| seen.insert(id.clone()))
                .collect::<Vec<_>>();
            for id in &ids {
                if !tx.query_row(
                    "SELECT EXISTS(SELECT 1 FROM track_versions WHERE id=?1)",
                    [id],
                    |r| r.get::<_, bool>(0),
                )? {
                    return Err(invalid("音轨已变化，请重新选择"));
                }
            }
            let sort_order = tx.query_row(
                "SELECT COALESCE(MAX(sort_order),-1)+1 FROM playlists",
                [],
                |r| r.get(0),
            )?;
            let p = Playlist {
                id: uuid::Uuid::new_v4().to_string(),
                name: name.into(),
                version_ids: ids,
                sort_order,
            };
            put(&tx, &p)?;
            tx.commit()?;
            Ok(p)
        })
    }
    pub fn update_playlist(
        &self,
        id: &str,
        name: Option<&str>,
        add: &[String],
        remove: Option<&str>,
        target: Option<usize>,
        expected: Option<&[String]>,
        group_delta: Option<i64>,
    ) -> AppResult<Playlist> {
        self.with_conn(|c| {
            let tx = c.transaction()?;
            let mut groups = read_all(&tx)?;
            let index = groups
                .iter()
                .position(|p| p.id == id)
                .ok_or_else(|| invalid("歌单不存在，请刷新"))?;
            let p = &mut groups[index];
            if let Some(name) = name {
                p.name = name.into();
            }
            if let Some(version) = remove {
                if let Some(target) = target {
                    if expected != Some(p.version_ids.as_slice()) {
                        return Err(invalid("歌单顺序已更新，请刷新后重试"));
                    }
                    let index = p
                        .version_ids
                        .iter()
                        .position(|v| v == version)
                        .ok_or_else(|| invalid("音轨已移出歌单"))?;
                    if target >= p.version_ids.len() {
                        return Err(invalid("目标位置无效"));
                    }
                    let v = p.version_ids.remove(index);
                    p.version_ids.insert(target, v);
                } else {
                    p.version_ids.retain(|v| v != version);
                }
            }
            for version in add {
                if !tx.query_row(
                    "SELECT EXISTS(SELECT 1 FROM track_versions WHERE id=?1)",
                    [version],
                    |r| r.get::<_, bool>(0),
                )? {
                    return Err(invalid("音轨已变化，请重新选择"));
                }
                if !p.version_ids.contains(version) {
                    p.version_ids.push(version.clone());
                }
            }
            put(&tx, p)?;
            let result = p.clone();
            if let Some(delta) = group_delta {
                let target =
                    (index as i64 + delta).clamp(0, groups.len().saturating_sub(1) as i64) as usize;
                let p = groups.remove(index);
                groups.insert(target, p);
                for (order, p) in groups.iter_mut().enumerate() {
                    p.sort_order = order as i64;
                    put(&tx, p)?;
                }
            }
            tx.commit()?;
            Ok(result)
        })
    }
    pub fn delete_playlist(&self, id: &str) -> AppResult<()> {
        self.with_conn(|c| {
            c.execute("DELETE FROM playlists WHERE id=?1", [id])?;
            Ok(())
        })
    }
    pub fn playlist_tracks(
        &self,
        ids: Option<&[String]>,
        nsfw: bool,
    ) -> AppResult<Vec<PlaylistTrack>> {
        let all=self.with_conn(|c|{let mut s=c.prepare("SELECT v.id,t.work_id FROM track_versions v JOIN tracks t ON t.id=v.track_id ORDER BY t.work_id,t.sort_key,v.id")?;
            let rows=s.query_map([],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?)))?.collect::<Result<Vec<_>,_>>()?;Ok(rows)})?;
        let owners: BTreeMap<_, _> = all.into_iter().collect();
        let selected = ids
            .map(|v| v.to_vec())
            .unwrap_or_else(|| owners.keys().cloned().collect());
        let mut details = BTreeMap::new();
        let mut out = Vec::new();
        for id in selected {
            let mut entry = PlaylistTrack {
                version_id: id.clone(),
                title: "音轨待匹配".into(),
                work_title: "同步后暂未找到对应音源".into(),
                cover_ref: None,
                available: false,
            };
            if let Some(work) = owners.get(&id) {
                if !details.contains_key(work) {
                    details.insert(work.clone(), self.get_work_detail(work)?);
                }
                let detail = &details[work];
                if !nsfw && detail.nsfw {
                    entry.title = "内容已隐藏".into();
                    entry.work_title.clear();
                } else if let Some((track, version)) = detail
                    .tracks
                    .iter()
                    .find_map(|t| t.versions.iter().find(|v| v.id == id).map(|v| (t, v)))
                {
                    entry.title = track.title.clone();
                    entry.work_title = detail.title.clone();
                    entry.cover_ref = detail.cover_ref.clone();
                    entry.available = version.association_state == "confirmed"
                        && version.sources.iter().any(|s| {
                            s.availability == "available"
                                && !s
                                    .absolute_path
                                    .as_deref()
                                    .is_some_and(|p| p.to_lowercase().ends_with(".part"))
                        });
                }
            }
            out.push(entry);
        }
        Ok(out)
    }
}
