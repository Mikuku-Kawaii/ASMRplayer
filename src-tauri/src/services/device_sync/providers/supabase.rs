use super::{fail, payload, read, save, AppResult, Client, Document, OAuth, SyncProvider, Value};
use parking_lot::Mutex;
use serde_json::json;
use std::{
    collections::HashMap,
    sync::{mpsc, LazyLock, OnceLock},
    time::Duration,
};
use tauri::Emitter;

pub struct SupabaseProvider;
pub(crate) static APP: OnceLock<tauri::AppHandle> = OnceLock::new();
static PENDING: LazyLock<Mutex<HashMap<String, mpsc::Sender<Value>>>> =
    LazyLock::new(Default::default);

// Only the main window hosts the official JS SDK. The caller keeps the sync
// service's operation lock throughout the exchange, including background runs.
fn request(request: Value) -> AppResult<Value> {
    let app = APP.get().ok_or_else(|| fail("播放器尚未就绪"))?;
    let id = uuid::Uuid::new_v4().to_string();
    let (tx, rx) = mpsc::channel();
    PENDING.lock().insert(id.clone(), tx);
    let result = app
        .emit_to("main", "sync:supabase", json!({"id":id,"request":request}))
        .map_err(|_| fail("无法启动 Supabase SDK"))
        .and_then(|_| {
            rx.recv_timeout(Duration::from_secs(60))
                .map_err(|_| fail("Supabase 请求超时"))
        });
    PENDING.lock().remove(&id);
    let result = result?;
    if let Some(error) = result["error"].as_str() {
        return Err(fail(error));
    }
    result
        .get("data")
        .cloned()
        .ok_or_else(|| fail("Supabase 响应无效"))
}
pub(crate) fn reply(id: &str, result: Value) {
    if let Some(tx) = PENDING.lock().remove(id) {
        let _ = tx.send(result);
    }
}
fn store_session(entry: &keyring::Entry, session: &Value) -> AppResult<()> {
    save(
        entry,
        session["refreshToken"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| fail("登录凭据无效"))?,
    )
}
impl SyncProvider for SupabaseProvider {
    fn oauth(&self) -> Option<&OAuth> {
        None
    }
    fn login(&self, entry: &keyring::Entry, input: &Value) -> AppResult<()> {
        let action = if input["signup"] == true {
            "signup"
        } else {
            "login"
        };
        let session =
            request(json!({"action":action,"email":input["email"],"password":input["password"]}))?;
        store_session(entry, &session)
    }
    fn access_token(&self, _: &Client, entry: &keyring::Entry) -> AppResult<String> {
        let token = read(entry)?.ok_or_else(|| fail("请先登录 Supabase"))?;
        let data = request(json!({"action":"refresh","refreshToken":token}))?;
        store_session(entry, &data)?;
        Ok(json!({"userId":data["userId"],"accessToken":data["accessToken"]}).to_string())
    }
    fn download_sync_data(
        &self,
        _: &Client,
        token: &str,
        _: &str,
        doc: &mut Document,
    ) -> AppResult<Option<String>> {
        let session: Value = super::super::codec::from_str(token)?;
        let mut cursor = String::new();
        loop {
            let row = request(json!({"action":"select","session":session,"after":cursor}))?;
            if row == json!({}) {
                break;
            }
            let device = row["device_id"]
                .as_str()
                .ok_or_else(|| fail("云端设备 ID 无效"))?;
            if device <= cursor.as_str() {
                return Err(fail("云端设备分页无效"));
            }
            let data = serde_json::to_vec(&row["sync_data"]).map_err(|_| fail("同步快照无效"))?;
            super::merge(doc, &data)?;
            cursor = device.into();
        }
        Ok(None)
    }
    fn upload_sync_data(
        &self,
        _: &Client,
        token: &str,
        device: &str,
        doc: &Document,
        _: Option<&str>,
    ) -> AppResult<()> {
        let session: Value = super::super::codec::from_str(token)?;
        let _ = payload(doc)?;
        request(json!({"action":"upsert","session":session,"device":device,"document":doc}))?;
        Ok(())
    }
}
