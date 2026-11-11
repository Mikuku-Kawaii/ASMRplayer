use super::{catalog::Catalog, protocol::{Document, Values}, codec, fail, SyncService, ProviderId};
use crate::domain::AppResult;
use rusqlite::OptionalExtension;
use serde_json::{json, Value};

fn normalized(value: &Value) -> Value { let mut value=value.clone(); if let Some(obj)=value.as_object_mut(){obj.remove("deviceName");} value }
pub fn describe(doc: &Document, before: &Values, catalog: &Catalog, device: &str) -> (Value, Option<Value>) {
    let targets = catalog.targets();
    let selected = doc.selected();
    let kinds = [("library", "音频库作品"), ("work", "喜欢作品"), ("track", "收藏音轨"),
        ("position", "收听进度"), ("bookmark", "时间标记"), ("playlist", "歌单")];
    let summary: Vec<_> = kinds.iter().map(|(kind, label)| {
        let rows: Vec<_> = selected.iter().filter(|(k,_)|k.starts_with(&format!("{kind}|"))).collect();
        json!({"kind":kind,"label":label,
            "total":rows.iter().filter(|(_,v)|!v.is_null() && **v != Value::Bool(false)).count(),
            "received":rows.iter().filter(|(k,v)|before.get(*k).map(normalized).unwrap_or(Value::Null)!=normalized(v) && doc.records[*k].last().is_some_and(|r|r.device!=device)).count(),
            "pending":rows.iter().filter(|(k,_)|*kind!="playlist" && !targets.contains(k.split('|').nth(1).unwrap_or(""))).count()})
    }).collect();
    let pending: Vec<_> = selected.iter().filter(|(k,_)|!k.starts_with("playlist|") && !targets.contains(k.split('|').nth(1).unwrap_or("")))
        .take(50).map(|(k,_)|{
            let target=k.split('|').nth(1).unwrap_or("");
            let label=catalog.names.get(target).cloned().or_else(||doc.online_works.get(target).map(|w|w.title.clone()))
                .or_else(||doc.online_works.values().find_map(|w|w.tracks.get(target).map(|t|format!("{} · {}",w.title,t.path)))).unwrap_or_else(||"尚未匹配的本地内容".into());
            json!({"key":k,"label":label,"kind":k.split('|').next(),"reason":"未找到相同音源，请导入原文件或补全在线作品"})
        }).collect();
    let candidate=doc.records.iter().filter_map(|(key,rows)| {
        if !key.starts_with("position|") || rows.len()!=1 { return None; }
        let r=&rows[0]; let v=&r.value; let time=v["updatedMs"].as_i64().unwrap_or(0);
        if r.device==device || v["completed"]==true || v["positionMs"].as_i64().unwrap_or(0)<=0 ||
            time<=before.get(key).and_then(|v|v["updatedMs"].as_i64()).unwrap_or(0) {return None;}
        let target=key.split('|').nth(1)?;
        let version=catalog.versions.iter().find(|(_,v)|v.as_str()==target)?.0;
        Some(json!({"id":format!("{}:{}:{}",r.device,r.seq,target),"versionId":version,
            "title":catalog.names.get(target).map(String::as_str).unwrap_or("同步音轨"),
            "positionMs":v["positionMs"],"updatedMs":time,
            "deviceName":v["deviceName"].as_str().filter(|s|s.chars().count()<=80).unwrap_or("其他设备")}))
    }).max_by_key(|v|v["updatedMs"].as_i64().unwrap_or(0));
    (json!({"summary":summary,"pendingItems":pending}),candidate)
}

impl SyncService {
    pub fn resume_candidate(&self, expected: Option<ProviderId>, id: Option<&str>) -> AppResult<Option<Value>> {
        let provider=self.check_selected(expected)?;
        let value=self.db.get_setting(&provider.setting("device_sync_resume"))?
            .and_then(|s|serde_json::from_str::<Value>(&s).ok()).filter(|v|!v.is_null());
        let Some(value)=value else { return Ok(None) };
        if id.is_some_and(|id|value["id"]!=id) {return Err(fail("收听位置已更新，请重试"));}
        let current=self.db.with_conn(|c|Ok(c.query_row("SELECT position_ms,completed,updated_at FROM playback_positions WHERE version_id=?1",
            [value["versionId"].as_str().unwrap_or("")],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,bool>(1)?,r.get::<_,String>(2)?))).optional()?))?;
        if current.is_none_or(|(_,done,time)| done || chrono::DateTime::parse_from_rfc3339(&time).map(|t|t.timestamp_millis()>value["updatedMs"].as_i64().unwrap_or(0)).unwrap_or(false)) {return Ok(None);}
        Ok(Some(value))
    }
    pub fn dismiss_resume(&self, expected:Option<ProviderId>, id:&str)->AppResult<()> {
        let provider=self.check_selected(expected)?;
        let _guard=self.gate.lock();
        let current=self.db.get_setting(&provider.setting("device_sync_resume"))?.and_then(|s|serde_json::from_str::<Value>(&s).ok());
        if current.as_ref().is_some_and(|v|v["id"]==id) {self.db.set_setting(&provider.setting("device_sync_resume"),&codec::to_string(&Value::Null)?)?;}
        Ok(())
    }
}
