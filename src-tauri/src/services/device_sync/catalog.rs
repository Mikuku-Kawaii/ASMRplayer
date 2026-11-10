use super::{
    fail,
    protocol::{Journal, Values},
};
use crate::{db::Database, domain::AppResult};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Read,
};

pub fn hash(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
#[derive(Default, Serialize, Deserialize)]
struct Fingerprint {
    size: u64,
    modified: u64,
    sha: String,
}
pub struct Catalog {
    pub versions: BTreeMap<String, String>,
    pub works: BTreeMap<String, String>,
    pub names: BTreeMap<String, String>,
}
impl Catalog {
    // An explicitly verified online descriptor can retain an existing file identity.
    // Include playlist members because their identities live inside the playlist value.
    pub fn use_verified_keys(&mut self, doc: &super::protocol::Document) {
        let mut recorded=doc.records.keys().filter_map(|k|k.split('|').nth(1).map(str::to_owned)).collect::<BTreeSet<_>>();
        recorded.extend(super::playlist_sync::referenced(&doc.selected()));
        let mut replacements=BTreeMap::new();
        for (key,work) in &doc.online_works {
            if let Some(local)=work.local_key.as_ref().filter(|local|recorded.contains(*local)&&!recorded.contains(key)){replacements.insert(key.clone(),local.clone());}
            for (key,track) in &work.tracks {if let Some(local)=track.local_key.as_ref().filter(|local|recorded.contains(*local)&&!recorded.contains(key)){replacements.insert(key.clone(),local.clone());}}
        }
        let mut counts=BTreeMap::new();for local in replacements.values(){*counts.entry(local.clone()).or_insert(0)+=1;}
        replacements.retain(|_,local|counts[local]==1);
        for map in [&mut self.works,&mut self.versions] {for value in map.values_mut(){if let Some(local)=replacements.get(value){*value=local.clone();}}}
    }
    pub fn describe_likes(
        &self,
        c: &Connection,
        doc: &mut super::protocol::Document,
    ) -> AppResult<()> {
        let mut tracked: BTreeSet<String> = doc
            .records
            .keys()
            .filter_map(|k| k.split('|').nth(1).map(str::to_owned))
            .collect();
        tracked.extend(super::playlist_sync::referenced(&doc.selected()));
        let mut by_work: BTreeMap<String, BTreeMap<String, super::protocol::OnlineTrack>> =
            BTreeMap::new();
        let mut media=c.prepare("SELECT t.work_id,s.version_id,s.source_work_id,s.remote_file_id,s.remote_relative_path,s.byte_size FROM media_sources s JOIN track_versions v ON v.id=s.version_id JOIN tracks t ON t.id=v.track_id WHERE s.provider LIKE 'asmr_one%' AND s.source_work_id IS NOT NULL AND s.remote_file_id IS NOT NULL AND s.remote_relative_path IS NOT NULL")?;
        let mut refs = media.query([])?;
        while let Some(r) = refs.next()? {
            let version: String = r.get(1)?;
            let Some(key) = self
                .versions
                .get(&version)
                .filter(|k| tracked.contains(k.as_str()))
            else {
                continue;
            };
            let source: String = r.get(2)?;
            let track = super::protocol::OnlineTrack {
                local_key: None,
                file_id: r.get(3)?,
                path: r.get(4)?,
                bytes: r.get::<_, Option<i64>>(5)?.and_then(|v| v.try_into().ok()),
            };
            if track.valid(source.trim_start_matches('0'), key) {
                by_work
                    .entry(r.get(0)?)
                    .or_default()
                    .insert(key.clone(), track);
            }
        }
        let mut s = c.prepare("SELECT w.id,s.source_work_id,w.title,w.nsfw_detected,(w.id IN (SELECT work_id FROM favorites) OR w.id IN (SELECT work_id FROM collected_works)) FROM works w JOIN work_sources s ON s.work_id=w.id WHERE s.provider LIKE 'asmr_one%'")?;
        let rows = s.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                super::protocol::OnlineWork {
                    source_id: r.get::<_, String>(1)?.trim_start_matches('0').into(),
                    title: r.get(2)?,
                    rating: if r.get::<_, bool>(3)? {
                        "NSFW"
                    } else {
                        "UNRATED"
                    }
                    .into(),
                    tracks: Default::default(),
                    local_key: None,
                },
                r.get::<_, bool>(4)?,
            ))
        })?;
        for row in rows {
            let (id, mut work, listed) = row?;
            work.tracks = by_work.remove(&id).unwrap_or_default();
            if !listed && work.tracks.is_empty() {
                continue;
            }
            if let Some(key) = self.works.get(&id) {
                if work.valid(key) {
                    if let Some(known) = doc.online_works.get(key) {
                        work.complete_from(known);
                    }
                    doc.online_works.insert(key.clone(), work);
                }
            }
        }
        let verified: Option<String> = c
            .query_row(
                "SELECT value_json FROM app_settings WHERE key='device_sync_verified_online_works'",
                [],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(raw) = verified {
            let online_works = super::codec::from_str(&raw)?;
            doc.merge(&super::protocol::Document {
                online_works,
                ..Default::default()
            })
            .map_err(fail)?;
        }
        Ok(())
    }
    pub fn restore_likes(
        &mut self,
        c: &Connection,
        doc: &super::protocol::Document,
        values: &Values,
    ) -> AppResult<()> {
        let needed=super::playlist_sync::referenced(values);
        let now = chrono::Utc::now().to_rfc3339();
        for (key, w) in &doc.online_works {
            let wanted = ["work", "library"]
                .iter()
                .any(|kind| values.get(&format!("{kind}|{key}|")) == Some(&Value::Bool(true)));
            if !wanted && !w.tracks.iter().any(|(key,track)|needed.contains(key)||track.local_key.as_ref().is_some_and(|local|needed.contains(local))) {
                continue;
            }
            let id = if let Some((id,_))=self.works.iter().find(|(_,known)|*known==key||Some(*known)==w.local_key.as_ref()) {id.clone()}else{
            let id=uuid::Uuid::new_v4().to_string();
            c.execute("INSERT INTO works(id,title,rj_code,nsfw_detected,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?5)",params![id,w.title,format!("RJ{:0>8}",w.source_id),w.rating=="NSFW",now])?;
            c.execute("INSERT INTO work_sources(id,work_id,provider,source_work_id,refreshed_at) VALUES(?1,?2,'asmr_one',?3,?4)",params![uuid::Uuid::new_v4().to_string(),id,w.source_id,now])?;
            self.works.insert(id.clone(), key.clone());
            self.names.insert(key.clone(), w.title.clone());
            id};
            for (canonical,track) in w.tracks.iter().filter(|(key,track)|needed.contains(*key)||track.local_key.as_ref().is_some_and(|local|needed.contains(local))) {
                let track_key=if needed.contains(canonical){canonical}else{track.local_key.as_ref().unwrap_or(canonical)};
                if self.versions.values().any(|known|known==track_key){continue;}
                let tid=uuid::Uuid::new_v4().to_string();let vid=uuid::Uuid::new_v4().to_string();
                let title=track.path.rsplit('/').next().unwrap_or(&track.path);
                let format=title.rsplit('.').next().unwrap_or("audio").to_lowercase();
                let dir=track.path.rsplit_once('/').map(|(dir,_)|dir).unwrap_or("");
                c.execute("INSERT INTO tracks(id,work_id,title,logical_dir_path,group_label,sort_key,created_at) VALUES(?1,?2,?3,?4,?4,?5,?6)",params![tid,id,title,dir,track.path,now])?;
                c.execute("INSERT INTO track_versions(id,track_id,version_label,created_at) VALUES(?1,?2,?3,?4)",params![vid,tid,format,now])?;
                c.execute("INSERT INTO media_sources(id,version_id,source_kind,provider,source_work_id,remote_file_id,remote_relative_path,format,byte_size,availability,created_at,updated_at) VALUES(?1,?2,'online_provider','asmr_one',?3,?4,?5,?6,?7,'available',?8,?8)",params![uuid::Uuid::new_v4().to_string(),vid,w.source_id,track.file_id,track.path,format,track.bytes,now])?;
                self.versions.insert(vid,track_key.clone());self.names.insert(track_key.clone(),format!("{} · {title}",w.title));
            }
        }
        Ok(())
    }
    pub fn load(db: &Database) -> AppResult<Self> {
        let rows=db.with_conn(|c| { let mut s=c.prepare("SELECT s.id,s.version_id,t.work_id,s.provider,s.source_work_id,s.remote_file_id,s.absolute_path FROM media_sources s JOIN track_versions v ON v.id=s.version_id JOIN tracks t ON t.id=v.track_id ORDER BY s.source_kind DESC")?;
            let rows=s.query_map([],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,Option<String>>(3)?,r.get::<_,Option<String>>(4)?,r.get::<_,Option<String>>(5)?,r.get::<_,Option<String>>(6)?)))?.collect::<Result<Vec<_>,_>>()?; Ok(rows) })?;
        let mut cache: BTreeMap<String, Fingerprint> = db
            .get_setting("device_sync_fingerprints")?
            .and_then(|s| crate::services::device_sync::codec::from_str(&s).ok())
            .unwrap_or_default();
        let mut versions = BTreeMap::new();
        let mut work_online = BTreeMap::new();
        let mut groups: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        // Prefer a provider identity whenever a version has one, on both platforms.
        for (_, v, w, p, sw, r, _) in &rows {
            if let (Some(p), Some(sw), Some(r)) = (p, sw, r) {
                if p.starts_with("asmr_one") {
                    let sw = sw.trim_start_matches('0');
                    versions.insert(v.clone(), hash(format!("asmr_one|{sw}|{r}").as_bytes()));
                    work_online.insert(w.clone(), hash(format!("asmr_one|{sw}").as_bytes()));
                }
            }
        }
        for (id, v, _, _, _, _, path) in &rows {
            if versions.contains_key(v) {
                continue;
            }
            if let Some(path) = path {
                if let Ok(meta) = std::fs::metadata(path) {
                    if meta.is_file() && !path.ends_with(".part") {
                        let modified = meta
                            .modified()
                            .ok()
                            .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
                            .map(|m| m.as_millis() as u64)
                            .unwrap_or(0);
                        let valid = cache
                            .get(id)
                            .is_some_and(|f| f.size == meta.len() && f.modified == modified);
                        if !valid {
                            let mut f = std::fs::File::open(path)?;
                            let mut h = Sha256::new();
                            let mut b = [0; 65536];
                            loop {
                                let n = f.read(&mut b)?;
                                if n == 0 {
                                    break;
                                }
                                h.update(&b[..n]);
                            }
                            cache.insert(
                                id.clone(),
                                Fingerprint {
                                    size: meta.len(),
                                    modified,
                                    sha: hex::encode(h.finalize()),
                                },
                            );
                        }
                    }
                }
            }
            if let Some(f) = cache.get(id) {
                versions.insert(v.clone(), f.sha.clone());
            }
        }
        for (_, v, w, _, _, _, _) in rows {
            if let Some(k) = versions.get(&v) {
                groups.entry(w).or_default().insert(k.clone());
            }
        }
        db.with_conn(|c| {
            let mut s = c.prepare(
                "SELECT work_id,source_work_id FROM work_sources WHERE provider LIKE 'asmr_one%'",
            )?;
            for row in s.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
                let (w, id) = row?;
                work_online.insert(
                    w,
                    hash(format!("asmr_one|{}", id.trim_start_matches('0')).as_bytes()),
                );
            }
            Ok(())
        })?;
        let mut works: BTreeMap<String, String> = groups
            .into_iter()
            .map(|(w, vs)| {
                let key = work_online.get(&w).cloned().unwrap_or_else(|| {
                    hash(
                        format!("files|{}", vs.into_iter().collect::<Vec<_>>().join("|"))
                            .as_bytes(),
                    )
                });
                (w, key)
            })
            .collect();
        works.extend(work_online);
        let aliases: Value = db
            .get_setting("device_sync_aliases")?
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or(json!({}));
        db.with_conn(|c| {
            for (table, field, map) in [
                ("works", "works", &mut works),
                ("track_versions", "versions", &mut versions),
            ] {
                let mut statement = c.prepare(&format!("SELECT id FROM {table}"))?;
                for id in statement.query_map([], |r| r.get::<_, String>(0))? {
                    let id = id?;
                    if let Some(key) = aliases[field][&id].as_str() {
                        map.insert(id, key.into());
                    }
                }
            }
            Ok(())
        })?;

        db.set_setting(
            "device_sync_fingerprints",
            &crate::services::device_sync::codec::to_string(&cache)?,
        )?;
        let names=db.with_conn(|c| {
            let mut names=BTreeMap::new();
            let mut works_query=c.prepare("SELECT id,title FROM works")?;
            for row in works_query.query_map([],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?)))? {let(id,title)=row?;if let Some(key)=works.get(&id){names.insert(key.clone(),title);}}
            let mut tracks_query=c.prepare("SELECT v.id,w.title,t.title FROM track_versions v JOIN tracks t ON t.id=v.track_id JOIN works w ON w.id=t.work_id")?;
            for row in tracks_query.query_map([],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?)))? {let(id,work,title)=row?;if let Some(key)=versions.get(&id){names.insert(key.clone(),format!("{work} · {title}"));}}
            Ok(names)
        })?;
        Ok(Self {
            versions,
            works,
            names,
        })
    }
    pub fn targets(&self) -> BTreeSet<String> {
        self.versions
            .values()
            .chain(self.works.values())
            .cloned()
            .collect()
    }
    pub fn snapshot(&self, c: &Connection) -> AppResult<Values> {
        let mut values = Values::new();
        for (id, k) in &self.works {
            let f: bool = c.query_row(
                "SELECT EXISTS(SELECT 1 FROM favorites WHERE work_id=?1)",
                [id],
                |r| r.get(0),
            )?;
            values.insert(format!("work|{k}|"), json!(f));
            let collected: bool = c.query_row(
                "SELECT EXISTS(SELECT 1 FROM collected_works WHERE work_id=?1)",
                [id],
                |r| r.get(0),
            )?;
            values.insert(format!("library|{k}|"), json!(collected));
        }
        for (id, k) in &self.versions {
            let f: bool = c.query_row(
                "SELECT EXISTS(SELECT 1 FROM favorite_versions WHERE version_id=?1)",
                [id],
                |r| r.get(0),
            )?;
            values.insert(format!("track|{k}|"), json!(f));
            let position=c.query_row("SELECT p.position_ms,COALESCE(v.duration_ms,0),p.completed,p.updated_at FROM playback_positions p JOIN track_versions v ON v.id=p.version_id WHERE p.version_id=?1",[id],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,i64>(1)?,r.get::<_,bool>(2)?,r.get::<_,String>(3)?))).optional()?;
            if let Some((pos, dur, done, time)) = position {
                values.insert(format!("position|{k}|"),json!({"positionMs":pos,"durationMs":dur,"completed":done,"updatedMs":millis(&time)}));
            }
            let mut s = c.prepare(
                "SELECT id,position_ms,note,created_at FROM bookmarks WHERE version_id=?1",
            )?;
            for row in s.query_map([id], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                ))
            })? {
                let (id, pos, note, time) = row?;
                values.insert(
                    format!("bookmark|{k}|{id}"),
                    json!({"positionMs":pos,"note":note,"createdMs":millis(&time)}),
                );
            }
        }
        values.extend(super::playlist_sync::snapshot(c,&self.versions)?);
        Ok(values)
    }
    pub fn apply(&self, c: &Connection, values: &Values) -> AppResult<usize> {
        super::playlist_sync::apply(c,values,&self.versions)?;
        let now = chrono::Utc::now().to_rfc3339();
        let mut count = 0;
        for (key, value) in values {
            let p: Vec<_> = key.split('|').collect();
            if p.len() != 3 {
                continue;
            }
            let ids = if matches!(p[0], "work" | "library") {
                &self.works
            } else {
                &self.versions
            };
            for (id, _) in ids.iter().filter(|(_, k)| k.as_str() == p[1]) {
                match p[0] {
                    "work" | "library" | "track" => {
                        let (table, column) = match p[0] {
                            "work" => ("favorites", "work_id"),
                            "library" => ("collected_works", "work_id"),
                            _ => ("favorite_versions", "version_id"),
                        };
                        if value == &Value::Bool(true) {
                            c.execute(&format!("INSERT OR IGNORE INTO {table} ({column},created_at) VALUES (?1,?2)"),params![id,now])?;
                        } else {
                            c.execute(&format!("DELETE FROM {table} WHERE {column}=?1"), [id])?;
                        }
                    }
                    "position" => {
                        let time = iso(value["updatedMs"].as_i64().unwrap_or(0));
                        c.execute("INSERT INTO playback_positions(version_id,position_ms,completed,updated_at) VALUES (?1,?2,?3,?4) ON CONFLICT(version_id) DO UPDATE SET position_ms=excluded.position_ms,completed=excluded.completed,updated_at=excluded.updated_at",params![id,value["positionMs"].as_i64(),value["completed"].as_bool(),time])?;
                        c.execute("INSERT INTO work_playback_state(work_id,last_version_id,last_played_at,updated_at) SELECT t.work_id,?1,?2,?2 FROM track_versions v JOIN tracks t ON t.id=v.track_id WHERE v.id=?1 ON CONFLICT(work_id) DO UPDATE SET last_version_id=excluded.last_version_id,last_played_at=excluded.last_played_at,updated_at=excluded.updated_at WHERE julianday(excluded.last_played_at)>=julianday(work_playback_state.last_played_at)",params![id,time])?;
                    }
                    "bookmark" if value.is_null() => {
                        c.execute(
                            "DELETE FROM bookmarks WHERE id=?1 AND version_id=?2",
                            params![p[2], id],
                        )?;
                    }
                    "bookmark" => {
                        c.execute("INSERT INTO bookmarks(id,version_id,position_ms,note,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,?6) ON CONFLICT(id) DO UPDATE SET position_ms=excluded.position_ms,note=excluded.note,updated_at=excluded.updated_at WHERE bookmarks.version_id=excluded.version_id",params![p[2],id,value["positionMs"].as_i64(),value["note"].as_str(),iso(value["createdMs"].as_i64().unwrap_or(0)),now])?;
                    }
                    _ => continue,
                }
                count += 1;
            }
        }
        Ok(count)
    }
}
fn millis(s: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(s)
        .map(|d| d.timestamp_millis().max(0))
        .unwrap_or(0)
}
fn iso(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms)
        .unwrap_or_default()
        .to_rfc3339()
}
pub fn read_journal(c: &Connection) -> AppResult<Journal> {
    let raw: Option<String> = c
        .query_row(
            "SELECT value_json FROM app_settings WHERE key='device_sync_journal'",
            [],
            |r| r.get(0),
        )
        .optional()?;
    match raw {
        Some(s) => crate::services::device_sync::codec::from_str(&s)
            .map_err(|_| fail("同步日志损坏，请从备份恢复")),
        None => Ok(Journal::default()),
    }
}
pub fn save_journal(c: &Connection, j: &Journal) -> AppResult<()> {
    c.execute("INSERT INTO app_settings(key,value_json,updated_at) VALUES('device_sync_journal',?1,?2) ON CONFLICT(key) DO UPDATE SET value_json=excluded.value_json,updated_at=excluded.updated_at",params![crate::services::device_sync::codec::to_string(j)?,chrono::Utc::now().to_rfc3339()])?;
    Ok(())
}
