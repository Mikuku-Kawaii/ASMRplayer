//! Cross-platform data sync, schema 1. Only provider-relative media paths travel.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

pub const LIMIT: usize = 16 * 1024 * 1024;

pub type Values = BTreeMap<String, Value>;
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Revision {
    pub device: String,
    pub seq: u64,
    pub clock: u64,
    pub context: BTreeMap<String, u64>,
    pub value: Value,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(try_from = "WireDocument", into = "WireDocument")]
pub struct Document {
    pub schema: u32,
    pub records: BTreeMap<String, Vec<Revision>>,
    pub online_works: BTreeMap<String, OnlineWork>,
}
// Schema 1 clients ignore optional fields. Keep membership out of their legacy records.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireDocument {
    schema: u32,
    records: BTreeMap<String, Vec<Revision>>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    library_records: BTreeMap<String, Vec<Revision>>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    playlist_records: BTreeMap<String, Vec<Revision>>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    online_works: BTreeMap<String, OnlineWork>,
}
impl TryFrom<WireDocument> for Document {
    type Error = &'static str;
    fn try_from(mut wire: WireDocument) -> Result<Self, Self::Error> {
        if wire
            .library_records
            .keys()
            .any(|k| !k.starts_with("library|"))
        {
            return Err("音频库记录无效");
        }
        wire.records.append(&mut wire.library_records);
        if wire.playlist_records.keys().any(|k|!k.starts_with("playlist|")){return Err("歌单记录无效");}
        wire.records.append(&mut wire.playlist_records);
        Ok(Self {
            schema: wire.schema,
            records: wire.records,
            online_works: wire.online_works,
        })
    }
}
impl From<Document> for WireDocument {
    fn from(doc: Document) -> Self {
        let (library_records, records): (BTreeMap<_,_>,BTreeMap<_,_>) = doc
            .records
            .into_iter()
            .partition(|(k, _)| k.starts_with("library|"));
        let (playlist_records, records)=records.into_iter().partition(|(k,_)|k.starts_with("playlist|"));
        Self {
            schema: doc.schema,
            records,
            library_records,
            playlist_records,
            online_works: doc.online_works,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OnlineWork {
    pub source_id: String,
    pub title: String,
    pub rating: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tracks: BTreeMap<String, OnlineTrack>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_key: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OnlineTrack {
    pub file_id: String,
    pub path: String,
    pub bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_key: Option<String>,
}
fn valid_local_key(key: Option<&str>) -> bool {
    key.is_none_or(|s| {
        s.len() == 64
            && s.bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    })
}
impl OnlineTrack {
    pub fn valid(&self, source: &str, key: &str) -> bool {
        valid_local_key(self.local_key.as_deref())
            && !self.file_id.is_empty()
            && self.file_id.len() <= 8192
            && self.path.len() <= 4096
            && !self.path.contains(['\\', '\0'])
            && !self
                .path
                .split('/')
                .any(|p| p.is_empty() || p == "." || p == "..")
            && self.bytes.is_none_or(|v| v <= i64::MAX as u64)
            && super::catalog::hash(format!("asmr_one|{source}|{}", self.file_id).as_bytes()) == key
    }
}
impl OnlineWork {
    pub fn complete_from(&mut self, other: &Self) {
        self.fill_unknown_rating(other);
        if self.local_key.is_none() {
            self.local_key.clone_from(&other.local_key);
        }
        for (key, track) in &other.tracks {
            self.tracks
                .entry(key.clone())
                .and_modify(|known| {
                    if known.local_key.is_none() {
                        known.local_key.clone_from(&track.local_key)
                    }
                })
                .or_insert_with(|| track.clone());
        }
    }
    pub fn fill_unknown_rating(&mut self, other: &Self) {
        if self.rating == "UNRATED" {
            self.rating.clone_from(&other.rating);
        }
    }
    pub fn valid(&self, key: &str) -> bool {
        valid_local_key(self.local_key.as_deref())
            && !self.source_id.is_empty()
            && self.source_id.len() <= 12
            && !self.source_id.starts_with('0')
            && self.source_id.bytes().all(|b| b.is_ascii_digit())
            && !self.title.is_empty()
            && self.title.len() <= 4096
            && matches!(self.rating.as_str(), "SAFE" | "NSFW" | "UNRATED")
            && self.tracks.len() <= 50_000
            && self.tracks.iter().all(|(k, t)| t.valid(&self.source_id, k))
            && super::catalog::hash(format!("asmr_one|{}", self.source_id).as_bytes()) == key
    }
}
impl Default for Document {
    fn default() -> Self {
        Self {
            schema: 1,
            records: BTreeMap::new(),
            online_works: BTreeMap::new(),
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Journal {
    pub device: String,
    pub doc: Document,
    pub baseline: Values,
}
impl Default for Journal {
    fn default() -> Self {
        Self {
            device: uuid::Uuid::new_v4().to_string(),
            doc: Document::default(),
            baseline: Values::new(),
        }
    }
}
impl Document {
    pub fn validate(&self) -> Result<(), String> {
        if self.online_works.len() > 100_000 || self.online_works.iter().any(|(k, w)| !w.valid(k)) {
            return Err("在线作品身份无效".into());
        }
        if self.schema != 1 || self.records.len() > 100_000 {
            return Err("不支持的同步格式或记录过多".into());
        }
        for (key, versions) in &self.records {
            let p: Vec<_> = key.split('|').collect();
            if p.len() != 3
                || !["work", "library", "track", "position", "bookmark", "playlist"].contains(&p[0])
                || p[1].len() != 64
                || !p[1].bytes().all(|b| b.is_ascii_hexdigit())
                || p[2].len() > 128
                || versions.is_empty()
                || versions.len() > 100
            {
                return Err("同步记录身份无效".into());
            }
            for r in versions {
                if r.device.is_empty()
                    || r.device.len() > 64
                    || r.seq == 0
                    || r.seq > 1_000_000_000_000
                    || r.clock > 1_000_000_000_000
                    || r.context.len() > 1000
                    || r.context.values().any(|v| *v > 1_000_000_000_000)
                {
                    return Err("同步修订无效".into());
                }
                let v = &r.value;
                let good = match p[0] {
                    "work" | "library" | "track" => v.is_boolean(),
                    "position" => {
                        ["positionMs", "durationMs", "updatedMs"]
                            .iter()
                            .all(|f| v[*f].as_u64().is_some_and(|n| n <= 9_000_000_000_000))
                            && v["completed"].is_boolean()
                    }
                    "bookmark" => {
                        v.is_null()
                            || (v["positionMs"]
                                .as_u64()
                                .is_some_and(|n| n <= 9_000_000_000_000)
                                && v["createdMs"].as_u64().is_some()
                                && v["note"].as_str().is_some_and(|s| s.len() <= 64_000))
                    }
                    "playlist" => super::playlist_sync::valid(p[1],p[2],v),
                    _ => false,
                };
                if !good {
                    return Err("同步记录内容无效".into());
                }
            }
        }
        Ok(())
    }
    pub fn vector(&self) -> BTreeMap<String, u64> {
        let mut v = BTreeMap::<String, u64>::new();
        for r in self.records.values().flatten() {
            for (d, n) in r.context.iter().chain(std::iter::once((&r.device, &r.seq))) {
                let e = v.entry(d.clone()).or_default();
                *e = (*e).max(*n);
            }
        }
        v
    }
    pub fn merge(&mut self, other: &Self) -> Result<(), String> {
        other.validate()?;
        for (key, work) in &other.online_works {
            self.online_works
                .entry(key.clone())
                .and_modify(|local| local.complete_from(work))
                .or_insert_with(|| work.clone());
        }
        for (key, incoming) in &other.records {
            let rows = self.records.entry(key.clone()).or_default();
            for r in incoming {
                if !rows.iter().any(|x| x.device == r.device && x.seq == r.seq) {
                    rows.push(r.clone());
                }
            }
            let all = rows.clone();
            rows.retain(|r| {
                !all.iter()
                    .any(|x| x.context.get(&r.device).copied().unwrap_or(0) >= r.seq)
            });
            rows.sort_by(|a, b| (a.clock, &a.device, a.seq).cmp(&(b.clock, &b.device, b.seq)));
        }
        self.validate()
    }
    pub fn selected(&self) -> Values {
        self.records
            .iter()
            .filter_map(|(k, v)| v.last().map(|r| (k.clone(), r.value.clone())))
            .collect()
    }
}
impl Journal {
    pub fn edit(&mut self, key: String, value: Value) {
        let context = self.doc.vector();
        let seq = context.get(&self.device).copied().unwrap_or(0) + 1;
        let clock = self
            .doc
            .records
            .values()
            .flatten()
            .map(|r| r.clock)
            .max()
            .unwrap_or(0)
            + 1;
        self.doc.records.insert(
            key,
            vec![Revision {
                device: self.device.clone(),
                seq,
                clock,
                context,
                value,
            }],
        );
    }
    pub fn capture(&mut self, current: &Values, targets: &std::collections::BTreeSet<String>) {
        let mut changes = Vec::new();
        for (key, v) in current {
            if self.baseline.get(key) != Some(v)
                && (self.baseline.contains_key(key) || v != &Value::Bool(false))
            {
                changes.push((key.clone(), v.clone()));
            }
        }
        for key in self.baseline.keys().cloned().collect::<Vec<_>>() {
            if !current.contains_key(&key)
                && (key.starts_with("playlist|") || (key.starts_with("bookmark|")
                && targets.contains(key.split('|').nth(1).unwrap_or(""))))
            {
                changes.push((key, Value::Null));
            }
        }
        let mut context = self.doc.vector();
        let mut clock = self
            .doc
            .records
            .values()
            .flatten()
            .map(|r| r.clock)
            .max()
            .unwrap_or(0);
        for (key, value) in changes {
            let seq = context.get(&self.device).copied().unwrap_or(0) + 1;
            clock += 1;
            self.doc.records.insert(
                key,
                vec![Revision {
                    device: self.device.clone(),
                    seq,
                    clock,
                    context: context.clone(),
                    value,
                }],
            );
            context.insert(self.device.clone(), seq);
        }
        self.baseline = current.clone();
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn key() -> String {
        format!("work|{}|", "a".repeat(64))
    }
    #[test]
    fn concurrent_edits_survive_and_resolution_dominates() {
        let mut a = Journal::default();
        let mut b = Journal::default();
        a.edit(key(), Value::Bool(true));
        b.doc.merge(&a.doc).unwrap();
        a.edit(key(), Value::Bool(false));
        b.edit(key(), Value::Bool(true));
        a.doc.merge(&b.doc).unwrap();
        assert_eq!(a.doc.records[&key()].len(), 2);
        a.edit(key(), Value::Bool(false));
        b.doc.merge(&a.doc).unwrap();
        assert_eq!(b.doc.records[&key()].len(), 1);
        assert_eq!(b.doc.selected()[&key()], false);
        b.doc.merge(&a.doc).unwrap();
        assert_eq!(b.doc.records[&key()].len(), 1);
    }
}
