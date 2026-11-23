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
    pub work_items: Option<Vec<WorkItem>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkItem {
    #[serde(
        default,
        rename = "fromSite",
        skip_serializing_if = "std::ops::Not::not"
    )]
    pub from_site: bool,
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub directory: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
}
impl WorkItem {
    pub fn new(id: String) -> Self {
        Self {
            id,
            directory: None,
            format: None,
            from_site: false,
        }
    }
    pub fn valid(&self) -> bool {
        !self.id.is_empty()
            && self.id.len() <= 128
            && self.directory.as_deref().is_none_or(|d| {
                d.len() <= 4096
                    && !d.starts_with(['/', '\\'])
                    && !d.contains([':', '\0'])
                    && !d.split(['/', '\\']).any(|s| s == "..")
            })
            && self.format.as_deref().is_none_or(|f| {
                !f.is_empty() && f.len() <= 16 && f.bytes().all(|b| b.is_ascii_alphanumeric())
            })
    }
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistTrack {
    pub version_id: String,
    pub title: String,
    pub work_title: String,
    pub work_id: Option<String>,
    pub track_id: Option<String>,
    pub work_track_count: usize,
    pub version_label: Option<String>,
    pub duration_ms: Option<i64>,
    pub cover_ref: Option<String>,
    pub available: bool,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistBackup {
    pub lists: Vec<Playlist>,
    #[serde(default)]
    pub online: BTreeMap<String, BackupWork>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupWork {
    source_id: String,
    title: String,
    code: Option<String>,
    nsfw: bool,
}
impl PlaylistBackup {
    pub fn export(c: &Connection) -> AppResult<Self> {
        let lists = read_all(c)?;
        let members: HashSet<_> = lists
            .iter()
            .flat_map(|p| p.work_items.iter().flatten().map(|w| w.id.as_str()))
            .collect();
        let mut online = BTreeMap::new();
        if !members.is_empty() {
            let mut stmt=c.prepare("SELECT w.id,s.source_work_id,w.title,w.rj_code,w.nsfw_detected FROM works w JOIN work_sources s ON s.work_id=w.id WHERE s.provider='asmr_one'")?;
            let mut rows = stmt.query([])?;
            while let Some(row) = rows.next()? {
                let id: String = row.get(0)?;
                if members.contains(id.as_str()) {
                    online.insert(
                        id,
                        BackupWork {
                            source_id: row.get(1)?,
                            title: row.get(2)?,
                            code: row.get(3)?,
                            nsfw: row.get(4)?,
                        },
                    );
                }
            }
        }
        Ok(Self { lists, online })
    }
    pub fn restore(&self, c: &Connection, now: &str) -> AppResult<usize> {
        use rusqlite::OptionalExtension;
        let mut mapped = BTreeMap::new();
        let members: HashSet<_> = self
            .lists
            .iter()
            .flat_map(|p| p.work_items.iter().flatten().map(|w| w.id.as_str()))
            .collect();
        for (id, entry) in &self.online {
            if !members.contains(id.as_str())
                || entry.source_id.is_empty()
                || entry.source_id.len() > 12
                || !entry.source_id.bytes().all(|b| b.is_ascii_digit())
                || entry.title.is_empty()
            {
                return Err(invalid("备份中的作品来源无效"));
            }
            let present:Option<String>=c.query_row("SELECT work_id FROM work_sources WHERE provider='asmr_one' AND source_work_id=?1",[&entry.source_id],|r|r.get(0)).optional()?;
            if let Some(known) = present {
                mapped.insert(id, known);
                continue;
            }
            c.execute("INSERT OR IGNORE INTO works(id,title,rj_code,nsfw_detected,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?5)",params![id,entry.title,entry.code,entry.nsfw,now])?;
            c.execute("INSERT INTO work_sources(id,work_id,provider,source_work_id,refreshed_at) VALUES(?1,?2,'asmr_one',?3,?4)",params![uuid::Uuid::new_v4().to_string(),id,entry.source_id,now])?;
        }
        let mut seen = HashSet::new();
        for p in &self.lists {
            if p.id.trim().is_empty()
                || p.id.len() > 128
                || p.id.contains('|')
                || !seen.insert(&p.id)
            {
                return Err(invalid("备份中的歌单标识无效"));
            }
            let mut p = p.clone();
            if let Some(items) = &mut p.work_items {
                for w in items {
                    if let Some(id) = mapped.get(&w.id) {
                        w.id = id.clone();
                    }
                }
            }
            put(c, &p)?;
        }
        Ok(self.lists.len())
    }
}

fn invalid(text: &str) -> AppError {
    AppError::new(ErrorCode::InvalidInput, text)
}
pub(crate) fn read_all(c: &Connection) -> AppResult<Vec<Playlist>> {
    let mut s = c.prepare(
        "SELECT id,name,version_ids,sort_order,work_items FROM playlists ORDER BY sort_order,id",
    )?;
    let rows = s
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, Option<String>>(4)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    rows.into_iter()
        .map(|(id, name, json, sort_order, works)| {
            Ok(Playlist {
                id,
                name,
                sort_order,
                work_items: works
                    .map(|raw| serde_json::from_str(&raw))
                    .transpose()
                    .map_err(|_| invalid("作品集内容无法读取"))?,
                version_ids: serde_json::from_str(&json)
                    .map_err(|_| invalid("歌单内容无法读取"))?,
            })
        })
        .collect()
}
/// Imported site names can exceed the local editor's 80-character limit.
pub(crate) fn valid_name(id: &str, name: &str) -> bool {
    let from_site = id.strip_prefix("asmr-list-").is_some_and(|hash| {
        hash.len() == 64
            && hash
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    });
    !name.trim().is_empty()
        && if from_site {
            name.encode_utf16().count() <= 8192
        } else {
            name.chars().count() <= 80
        }
}

pub(crate) fn put(c: &Connection, p: &Playlist) -> AppResult<()> {
    let name = p.name.trim();
    if let Some(items) = &p.work_items {
        if !p.version_ids.is_empty()
            || items.iter().any(|w| !w.valid())
            || items.iter().map(|w| &w.id).collect::<HashSet<_>>().len() != items.len()
        {
            return Err(invalid("作品集内容无效"));
        }
    }
    if !valid_name(&p.id, name)
        || p.version_ids.len() > 500
        || p.version_ids
            .iter()
            .any(|id| id.is_empty() || id.len() > 128)
    {
        return Err(invalid("歌单名称为 1–80 个字，每个歌单最多 500 首音轨"));
    }
    c.execute("INSERT INTO playlists(id,name,version_ids,sort_order,work_items) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(id) DO UPDATE SET name=excluded.name,version_ids=excluded.version_ids,sort_order=excluded.sort_order,work_items=excluded.work_items",
        params![p.id,name,serde_json::to_string(&p.version_ids).map_err(|_|invalid("歌单无法保存"))?,p.sort_order, p.work_items.as_ref().map(|items| serde_json::to_string(items).expect("serializable work entries"))])?;
    Ok(())
}
fn validate_works(c: &Connection, works: &[WorkItem]) -> AppResult<()> {
    let mut stmt = c.prepare_cached("SELECT EXISTS(SELECT 1 FROM works WHERE id=?1)")?;
    for w in works {
        if !w.valid() || !stmt.query_row([&w.id], |r| r.get::<_, bool>(0))? {
            return Err(invalid("作品已变化，请重新选择"));
        }
    }
    Ok(())
}
fn move_index(ids: &[String], id: &str, target: usize, expected: &[String]) -> AppResult<usize> {
    if ids != expected || target >= ids.len() {
        return Err(invalid("顺序已更新，请刷新后重试"));
    }
    ids.iter()
        .position(|item| item == id)
        .ok_or_else(|| invalid("条目已移出歌单"))
}
impl Database {
    pub fn list_playlists(&self) -> AppResult<Vec<Playlist>> {
        self.with_conn(|c| read_all(c))
    }
    pub fn create_playlist(&self, name: &str, ids: Vec<String>) -> AppResult<Playlist> {
        self.create_typed_playlist(name, ids, None)
    }
    pub fn create_typed_playlist(
        &self,
        name: &str,
        ids: Vec<String>,
        works: Option<Vec<WorkItem>>,
    ) -> AppResult<Playlist> {
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
            if let Some(items) = &works {
                validate_works(&tx, items)?;
            }
            let p = Playlist {
                work_items: works,
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
            if p.work_items.is_some() && (remove.is_some() || !add.is_empty()) {
                return Err(invalid("请选择音轨歌单"));
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
                let same: Vec<_> = groups
                    .iter()
                    .enumerate()
                    .filter(|(_, g)| g.work_items.is_some() == result.work_items.is_some())
                    .map(|(i, _)| i)
                    .collect();
                let pos = same.iter().position(|i| *i == index).unwrap();
                let target = same[(pos as i64 + delta).clamp(0, same.len() as i64 - 1) as usize];
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
    pub fn change_members(
        &self,
        id: &str,
        works: Option<&[WorkItem]>,
        tracks: &[String],
        remove: &[String],
        movement: Option<(&str, usize, &[String])>,
    ) -> AppResult<Vec<String>> {
        self.with_conn(|c| {
            let tx = c.transaction()?;
            let mut p = read_all(&tx)?
                .into_iter()
                .find(|p| p.id == id)
                .ok_or_else(|| invalid("歌单不存在"))?;
            let mut added = Vec::new();
            let removed: HashSet<_> = remove.iter().collect();
            match (&mut p.work_items, works) {
                (Some(members), Some(items)) => {
                    if !tracks.is_empty() {
                        return Err(invalid("请选择作品"));
                    }
                    validate_works(&tx, items)?;
                    if let Some((item, index, expected)) = movement {
                        let current = members.iter().map(|w| w.id.clone()).collect::<Vec<_>>();
                        let pos = move_index(&current, item, index, expected)?;
                        let entry = members.remove(pos);
                        members.insert(index, entry);
                    }
                    members.retain(|w| !removed.contains(&w.id));
                    let mut existing: HashSet<_> = members.iter().map(|w| w.id.clone()).collect();
                    for w in items {
                        if existing.insert(w.id.clone()) {
                            members.push(w.clone());
                            added.push(w.id.clone());
                        }
                    }
                }
                (None, None) => {
                    if let Some((item, target, expected)) = movement {
                        let from = move_index(&p.version_ids, item, target, expected)?;
                        let entry = p.version_ids.remove(from);
                        p.version_ids.insert(target, entry);
                    }
                    p.version_ids.retain(|v| !removed.contains(v));
                    for v in tracks {
                        if !tx.query_row(
                            "SELECT EXISTS(SELECT 1 FROM track_versions WHERE id=?1)",
                            [v],
                            |r| r.get::<_, bool>(0),
                        )? {
                            return Err(invalid("音轨已变化，请重新选择"));
                        }
                        if !p.version_ids.contains(v) {
                            p.version_ids.push(v.clone());
                            added.push(v.clone());
                        }
                    }
                }
                _ => return Err(invalid("歌单类型已变化，请重新选择")),
            }
            put(&tx, &p)?;
            tx.commit()?;
            Ok(added)
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
        let selected = ids
            .map(<[String]>::to_vec)
            .unwrap_or_else(|| all.iter().map(|(id, _)| id.clone()).collect());
        let owners: BTreeMap<_, _> = all.into_iter().collect();
        let mut details = BTreeMap::new();
        let mut out = Vec::new();
        for id in selected {
            let mut entry = PlaylistTrack {
                version_id: id.clone(),
                title: "音轨待匹配".into(),
                work_id: None,
                track_id: None,
                work_track_count: 0,
                version_label: None,
                duration_ms: None,
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
                    entry.work_id = Some(work.clone());
                    entry.track_id = Some(track.id.clone());
                    entry.work_track_count = detail.tracks.len();
                    entry.duration_ms = version.duration_ms;
                    entry.version_label = Some(version.version_label.clone());
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
