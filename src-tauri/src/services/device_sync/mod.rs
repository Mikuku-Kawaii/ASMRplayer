pub mod automatic;
mod catalog;
mod playlist_sync;
pub(crate) mod codec;
mod github_auth;
mod http;
mod source_enrichment;
mod oauth;
pub mod providers;
use providers::ProviderId;
pub mod protocol;
#[cfg(test)]
mod tests;
use crate::{
    db::Database,
    domain::{AppError, AppResult, ErrorCode},
};
use parking_lot::Mutex;
use protocol::Document;
use serde::Serialize;
use serde_json::{json, Value};
use std::sync::{atomic::Ordering, Arc};
use tauri::Emitter;
pub(super) fn fail(s: impl Into<String>) -> AppError {
    AppError::new(ErrorCode::InvalidInput, s.into())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub provider: ProviderId,
    pub configured: bool,
    pub connected: bool,
    pub auto_policy: automatic::AutoPolicy,
    pub auto_error: Option<String>,
    pub records: usize,
    pub pending: usize,
    pub conflicts: Vec<Value>,
    pub last_sync: Option<String>,
    pub metadata_pending: usize,
    pub metadata_error: Option<String>,
}
#[derive(Clone)]
pub struct SyncService {
    app: Arc<Mutex<Option<tauri::AppHandle>>>,
    db: Database,
    gate: Arc<Mutex<()>>,
    run_gate: Arc<Mutex<()>>,
    github_challenge: Arc<Mutex<Option<github_auth::Challenge>>>,
    github_cancel: Arc<std::sync::atomic::AtomicBool>,
    auto_attempt: Arc<std::sync::atomic::AtomicI64>,
    source:crate::providers::AsmrOneProvider,
}
impl SyncService {
    pub fn new(db: Database) -> Self {
        let source=crate::providers::AsmrOneProvider::new(db.clone());
        Self::with_source(db,source)
    }
    pub fn with_source(db:Database, source:crate::providers::AsmrOneProvider)->Self {
        // Remove the retired provider's local credential once per installation.
        if db
            .get_setting("device_sync_retired_credential_removed")
            .ok()
            .flatten()
            .is_none()
        {
            let profile = catalog::hash(db.data_dir.to_string_lossy().as_bytes());
            if matches!(
                keyring::Entry::new("ASMRPlayer.GoogleDrive", &profile)
                    .and_then(|entry| entry.delete_credential()),
                Ok(()) | Err(keyring::Error::NoEntry)
            ) {
                let _ = db.set_setting("device_sync_retired_credential_removed", "true");
            }
        }
        Self {
            db,
            source,
            app: Default::default(),
            gate: Default::default(),
            run_gate: Default::default(),
            github_challenge: Default::default(),
            github_cancel: Default::default(),
            auto_attempt: Arc::new(automatic::clock()),
        }
    }
    pub fn attach_app_handle(&self, app: tauri::AppHandle) {
        let _ = providers::supabase::APP.set(app.clone());
        *self.app.lock() = Some(app);
    }
    fn selected(&self) -> AppResult<ProviderId> {
        self.db
            .get_setting("device_sync_provider")?
            .filter(|s| s != "\"google\"")
            .map(|s| codec::from_str(&s))
            .transpose()
            .map(|v| v.unwrap_or_default())
    }
    fn check_selected(&self, expected: Option<ProviderId>) -> AppResult<ProviderId> {
        let id = self.selected()?;
        if expected.is_some_and(|v| v != id) {
            return Err(fail("存储方式已切换，请重试"));
        }
        Ok(id)
    }
    fn entry(&self, id: ProviderId) -> AppResult<keyring::Entry> {
        id.credential(&catalog::hash(
            self.db.data_dir.to_string_lossy().as_bytes(),
        ))
    }
    pub fn select(&self, id: ProviderId) -> AppResult<Status> {
        let _guard = self
            .run_gate
            .try_lock()
            .ok_or_else(|| fail("同步或授权正在进行"))?;
        self.db
            .set_setting("device_sync_provider", &codec::to_string(&id)?)?;
        self.auto_attempt
            .store(chrono::Utc::now().timestamp(), Ordering::Relaxed);
        self.status()
    }
    pub fn github_start(&self) -> AppResult<String> {
        let _guard = self
            .run_gate
            .try_lock()
            .ok_or_else(|| fail("同步或授权正在进行"))?;
        self.check_selected(Some(ProviderId::Github))?;
        let challenge = github_auth::begin()?;
        let code = challenge.code.clone();
        *self.github_challenge.lock() = Some(challenge);
        self.github_cancel.store(false, Ordering::Relaxed);
        Ok(code)
    }
    pub fn github_cancel(&self) {
        self.github_cancel.store(true, Ordering::Relaxed);
        self.github_challenge.lock().take();
    }
    pub fn login(&self, expected: Option<ProviderId>, input: &Value) -> AppResult<Status> {
        let _guard = self
            .run_gate
            .try_lock()
            .ok_or_else(|| fail("同步或授权正在进行"))?;
        let id = self.check_selected(expected)?;
        if id == ProviderId::Github && input["device"].as_bool() == Some(true) {
            let pending = self
                .github_challenge
                .lock()
                .take()
                .ok_or_else(|| fail("请重新申请 GitHub 授权码"))?;
            let stored = github_auth::finish(pending, &self.github_cancel)?;
            if self.github_cancel.load(Ordering::Relaxed) {
                return Err(fail("已取消 GitHub 登录"));
            }
            self.entry(id)?
                .set_password(&stored)
                .map_err(|_| fail("无法保存 GitHub 登录"))?;
        } else {
            id.provider().login(&self.entry(id)?, input)?;
        }
        self.db
            .set_setting("device_sync_provider", &codec::to_string(&id)?)?;
        self.auto_attempt
            .store(chrono::Utc::now().timestamp(), Ordering::Relaxed);
        if self
            .db
            .get_setting(&id.setting("device_sync_auto"))?
            .is_none()
        {
            let policy = automatic::AutoPolicy {
                periodic: true,
                ..Default::default()
            };
            self.db
                .set_setting(&id.setting("device_sync_auto"), &codec::to_string(&policy)?)?;
        }
        self.status()
    }
    pub fn logout(&self, expected: Option<ProviderId>) -> AppResult<Status> {
        let _operation = self
            .run_gate
            .try_lock()
            .ok_or_else(|| fail("同步或授权正在进行"))?;
        let id = self.check_selected(expected)?;
        id.provider().logout(&self.entry(id)?)?;
        self.status()
    }
    pub fn status(&self) -> AppResult<Status> {
        let j = self.db.with_conn(|c| catalog::read_journal(c))?;
        let names: std::collections::BTreeMap<String, String> = self
            .db
            .get_setting("device_sync_names")?
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        let conflicts = j
            .doc
            .records
            .iter()
            .filter(|(_, r)| r.len() > 1)
            .map(|(k, r)| json!({"key":k,"label":names.get(k.split('|').nth(1).unwrap_or("")).map(String::as_str).unwrap_or("尚未匹配的内容"),"choices":r}))
            .collect();

        let pending = self
            .db
            .get_setting("device_sync_pending")?
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let id = self.selected()?;
        let (configured, connected) = id.provider().get_status(&self.entry(id)?)?;
        Ok(Status {
            provider: id,
            configured,
            connected,
            auto_policy: self.auto_policy(id)?,
            auto_error: self.db.get_setting(&id.setting("device_sync_auto_error"))?,
            records: j.doc.records.len(),
            pending,
            conflicts,
            last_sync: self.db.get_setting(&id.setting("device_sync_last"))?,
            metadata_pending:self.db.get_setting("device_sync_metadata_pending")?.and_then(|s|s.parse().ok()).unwrap_or(0),
            metadata_error:self.db.get_setting("device_sync_metadata_error")?,
        })
    }
    fn exchange_local(
        &self,
        incoming: Option<&Document>,
        resolve: Option<(&str, &str, u64)>,
    ) -> AppResult<Document> {
        let _guard = self.gate.lock();
        let mut catalog = catalog::Catalog::load(&self.db)?;
        let doc = self.db.with_conn(|c|{let tx=c.transaction()?;let mut j=catalog::read_journal(&tx)?;catalog.use_verified_keys(&j.doc);j.capture(&catalog.snapshot(&tx)?,&catalog.targets());
            catalog.describe_likes(&tx,&mut j.doc)?;
            if let Some(doc)=incoming {j.doc.merge(doc).map_err(fail)?;}
            if let Some((key,device,seq))=resolve {let v=j.doc.records.get(key).and_then(|v|v.iter().find(|r|r.device==device && r.seq==seq)).ok_or_else(||fail("记录已变化，请重新同步后选择"))?.value.clone();j.edit(key.into(),v);}
            j.doc.validate().map_err(fail)?;catalog.use_verified_keys(&j.doc);let selected=j.doc.selected();catalog.restore_likes(&tx,&j.doc,&selected)?;catalog.apply(&tx,&selected)?;j.baseline=catalog.snapshot(&tx)?;
            let targets=catalog.targets();
            let pending=j.doc.records.keys().filter(|k|!k.starts_with("playlist|")&&!targets.contains(k.split('|').nth(1).unwrap_or(""))).count()+playlist_sync::referenced(&selected).difference(&targets).count();
            for (key,value) in selected.iter().filter(|(key,_)|key.starts_with("playlist|")){if let Some(name)=value["name"].as_str(){catalog.names.insert(key.split('|').nth(1).unwrap_or("").into(),format!("歌单 · {name}"));}}
            tx.execute("INSERT INTO app_settings(key,value_json,updated_at) VALUES('device_sync_pending',?1,'') ON CONFLICT(key) DO UPDATE SET value_json=excluded.value_json",[pending.to_string()])?;
            tx.execute("INSERT INTO app_settings(key,value_json,updated_at) VALUES('device_sync_names',?1,'') ON CONFLICT(key) DO UPDATE SET value_json=excluded.value_json",[codec::to_string(&catalog.names)?])?;
            catalog::save_journal(&tx,&j)?;tx.commit()?;Ok(j.doc)})?;
        if incoming.is_some() || resolve.is_some() {
            if let Some(app) = self.app.lock().as_ref() {
                let _ = app.emit("library:affinity", "");
            }
        }
        Ok(doc)
    }
    pub fn resolve(&self, key: &str, device: &str, seq: u64) -> AppResult<Status> {
        self.exchange_local(None, Some((key, device, seq)))?;
        self.status()
    }
    pub fn run(&self, expected: Option<ProviderId>) -> AppResult<Status> {
        let _operation = self
            .run_gate
            .try_lock()
            .ok_or_else(|| fail("同步正在进行"))?;
        self.auto_attempt
            .store(chrono::Utc::now().timestamp(), Ordering::Relaxed);
        let id = self.check_selected(expected)?;
        self.db
            .set_setting("device_sync_provider", &codec::to_string(&id)?)?;
        let provider = id.provider();
        let client = http::client()?;
        let access = provider.access_token(&client, &self.entry(id)?)?;
        let mut merged = self.exchange_local(None, None)?;
        let device = self
            .db
            .with_conn(|c| Ok(catalog::read_journal(c)?.device))?;
        provider.exchange_sync_data(&client, &access, &device, &mut merged)?;
        self.exchange_local(Some(&merged), None)?;
        self.complete_source_information()?;
        self.db
            .set_setting(&id.setting("device_sync_auto_error"), "")?;
        self.db.set_setting(
            &id.setting("device_sync_last"),
            &chrono::Utc::now().to_rfc3339(),
        )?;
        self.status()
    }
}
