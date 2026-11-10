use super::{
    fail, http,
    oauth::{self, OAuth, MICROSOFT},
    protocol::{Document, LIMIT},
};
use crate::domain::AppResult;
use reqwest::blocking::{Client, RequestBuilder, Response};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::Read;
use url::Url;
#[cfg(test)]
mod tests;
mod webdav;
pub(crate) mod supabase;

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ProviderId {
    #[default]
    Github,
    Onedrive,
    Webdav,
    Supabase,
}
impl ProviderId {
    pub fn provider(self) -> &'static dyn SyncProvider {
        match self {
            Self::Webdav => &webdav::WebDavProvider,
            Self::Supabase => &supabase::SupabaseProvider,

            Self::Github => &GitHubGistProvider { api: GISTS },
            Self::Onedrive => &OneDriveProvider { root: GRAPH },
        }
    }
    pub fn setting(self, key: &str) -> String {
        match self {
            Self::Webdav => format!("{key}_webdav"),
            Self::Supabase => format!("{key}_supabase"),

            Self::Github => format!("{key}_github"),
            Self::Onedrive => format!("{key}_onedrive"),
        }
    }
    pub fn credential(self, profile: &str) -> AppResult<keyring::Entry> {
        let service = match self {
            Self::Webdav => "ASMRPlayer.WebDAV",
            Self::Supabase => "ASMRPlayer.Supabase",

            Self::Github => "ASMRPlayer.GitHubGist",
            Self::Onedrive => "ASMRPlayer.OneDrive",
        };
        keyring::Entry::new(service, profile).map_err(|_| fail("无法打开系统凭据存储"))
    }
}

fn read(entry: &keyring::Entry) -> AppResult<Option<String>> {
    match entry.get_password() {
        Ok(t) => Ok(Some(t)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(_) => Err(fail("无法读取账号凭据")),
    }
}
fn save(entry: &keyring::Entry, t: &str) -> AppResult<()> {
    entry.set_password(t).map_err(|_| fail("无法保存账号凭据"))
}
pub trait SyncProvider: Sync {
    fn oauth(&self) -> Option<&OAuth>;
    fn get_status(&self, entry: &keyring::Entry) -> AppResult<(bool, bool)> {
        Ok((
            self.oauth().is_none_or(|o| o.id().is_ok()),
            read(entry)?.is_some(),
        ))
    }
    fn login(&self, entry: &keyring::Entry, _input: &Value) -> AppResult<()> {
        save(
            entry,
            &oauth::login(self.oauth().ok_or_else(|| fail("此方式需要访问令牌"))?)?,
        )
    }
    fn logout(&self, entry: &keyring::Entry) -> AppResult<()> {
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err(fail("无法清除账号凭据")),
        }
    }
    fn access_token(&self, client: &Client, entry: &keyring::Entry) -> AppResult<String> {
        let secret = read(entry)?.ok_or_else(|| fail("请先绑定当前存储账号"))?;
        if let Some(o) = self.oauth() {
            let (access, rotated) = oauth::access_token(o, client, &secret)?;
            if let Some(t) = rotated {
                save(entry, &t)?;
            }
            Ok(access)
        } else {
            Ok(secret)
        }
    }
    fn exchange_sync_data(
        &self,
        c: &Client,
        access: &str,
        device: &str,
        doc: &mut Document,
    ) -> AppResult<()> {
        let own = self.download_sync_data(c, access, device, doc)?;
        self.upload_sync_data(c, access, device, doc, own.as_deref())
    }
    fn download_sync_data(
        &self,
        c: &Client,
        t: &str,
        device: &str,
        doc: &mut Document,
    ) -> AppResult<Option<String>>;
    fn upload_sync_data(
        &self,
        c: &Client,
        t: &str,
        device: &str,
        doc: &Document,
        own: Option<&str>,
    ) -> AppResult<()>;
}
const GRAPH: &str = "https://graph.microsoft.com/v1.0/me/drive/special/approot";
const GISTS: &str = "https://api.github.com/gists";
const DESCRIPTION: &str = "ASMR Player sync v1";
fn filename(device: &str) -> String {
    format!("asmr-sync-v1-{device}.json")
}
fn snapshot_name(s: &str) -> bool {
    s.strip_prefix("asmr-sync-v1-")
        .and_then(|v| v.strip_suffix(".json"))
        .is_some_and(|v| uuid::Uuid::parse_str(v).is_ok())
}
fn send(req: RequestBuilder) -> AppResult<Response> {
    let res = req
        .send()
        .map_err(|_| fail("无法连接同步存储，请检查网络"))?;
    if !res.status().is_success() {
        return Err(fail(format!("同步存储返回 HTTP {}", res.status().as_u16())));
    }
    Ok(res)
}
fn bytes(response: Response, limit: usize) -> AppResult<Vec<u8>> {
    let mut data = Vec::new();
    response.take(limit as u64 + 1).read_to_end(&mut data)?;
    if data.len() > limit {
        return Err(fail("同步响应超过大小上限"));
    }
    Ok(data)
}
fn metadata(req: RequestBuilder) -> AppResult<Value> {
    serde_json::from_slice(&bytes(send(req)?, LIMIT * 2)?)
        .map_err(|_| fail("存储服务返回的数据无效"))
}
fn merge(doc: &mut Document, data: &[u8]) -> AppResult<()> {
    if data.len() > LIMIT {
        return Err(fail("同步文件超过大小上限"));
    }
    let other: Document = serde_json::from_slice(data).map_err(|_| fail("云端同步文件无效"))?;
    doc.merge(&other).map_err(fail)
}
fn payload(doc: &Document) -> AppResult<String> {
    let s = serde_json::to_string(doc).map_err(|_| fail("同步资料编码失败"))?;
    if s.len() > LIMIT {
        return Err(fail("同步资料超过大小上限"));
    }
    Ok(s)
}
fn https(s: &str, host: Option<&str>) -> AppResult<Url> {
    let url = Url::parse(s).map_err(|_| fail("存储服务返回无效链接"))?;
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || host.is_some_and(|h| url.host_str() != Some(h) || url.port().is_some())
    {
        return Err(fail("存储服务返回不受支持的下载地址"));
    }
    Ok(url)
}

struct GitHubGistProvider<'a> {
    api: &'a str,
}
impl SyncProvider for GitHubGistProvider<'_> {
    fn access_token(&self, c: &Client, entry: &keyring::Entry) -> AppResult<String> {
        let stored = read(entry)?.ok_or_else(|| fail("请先登录 GitHub"))?;
        let (access, updated) = super::github_auth::access(c, stored)?;
        if let Some(encoded) = updated {
            save(entry, &encoded)?;
        }
        Ok(access)
    }
    fn oauth(&self) -> Option<&OAuth> {
        None
    }
    fn login(&self, entry: &keyring::Entry, input: &Value) -> AppResult<()> {
        let t = input["token"].as_str().unwrap_or("").trim();
        if t.is_empty() {
            return Err(fail("请输入具有 Gist 读写权限的 GitHub 令牌"));
        }
        metadata(
            http::client()?
                .get("https://api.github.com/user")
                .bearer_auth(t),
        )?;
        save(entry, t)
    }
    fn download_sync_data(
        &self,
        c: &Client,
        t: &str,
        device: &str,
        doc: &mut Document,
    ) -> AppResult<Option<String>> {
        let own = filename(device);
        let mut own_id = None;
        for page in 1u32.. {
            let data = metadata(
                c.get(self.api)
                    .bearer_auth(t)
                    .query(&[("per_page", 100), ("page", page)]),
            )?;
            let list = data.as_array().ok_or_else(|| fail("Gist 列表格式无效"))?;
            for item in list {
                if item["public"] != false || item["description"] != DESCRIPTION {
                    continue;
                }
                let id = item["id"]
                    .as_str()
                    .filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_hexdigit()))
                    .ok_or_else(|| fail("Gist ID 无效"))?;
                let detail = metadata(c.get(format!("{}/{id}", self.api)).bearer_auth(t))?;
                if detail["public"] != false || detail["description"] != DESCRIPTION {
                    continue;
                }
                for (name, file) in detail["files"]
                    .as_object()
                    .ok_or_else(|| fail("Gist 文件清单无效"))?
                {
                    if !snapshot_name(name) {
                        continue;
                    }
                    if file["truncated"] == true {
                        let url = https(
                            file["raw_url"].as_str().unwrap_or(""),
                            Some("gist.githubusercontent.com"),
                        )?;
                        merge(doc, &bytes(send(c.get(url))?, LIMIT)?)?;
                    } else {
                        merge(
                            doc,
                            file["content"]
                                .as_str()
                                .ok_or_else(|| fail("Gist 内容缺失"))?
                                .as_bytes(),
                        )?
                    }
                    if *name == own {
                        own_id = Some(id.into())
                    }
                }
            }
            if list.len() < 100 {
                break;
            }
        }
        Ok(own_id)
    }
    fn upload_sync_data(
        &self,
        c: &Client,
        t: &str,
        device: &str,
        doc: &Document,
        own: Option<&str>,
    ) -> AppResult<()> {
        let content = payload(doc)?;
        if content.len() > 10_000_000 {
            return Err(fail("快照超过 Gist API 可下载的 10 MB，请改用其他存储"));
        }
        let files = json!({(filename(device)):{"content":content}});
        let req = if let Some(id) = own {
            c.patch(format!("{}/{id}", self.api))
                .json(&json!({"files":files}))
        } else {
            c.post(self.api)
                .json(&json!({"description":DESCRIPTION,"public":false,"files":files}))
        };
        send(req.bearer_auth(t))?;
        Ok(())
    }
}
struct OneDriveProvider<'a> {
    root: &'a str,
}
impl SyncProvider for OneDriveProvider<'_> {
    fn oauth(&self) -> Option<&OAuth> {
        Some(&MICROSOFT)
    }
    fn download_sync_data(
        &self,
        c: &Client,
        t: &str,
        _device: &str,
        doc: &mut Document,
    ) -> AppResult<Option<String>> {
        send(c.get(self.root).bearer_auth(t))?;
        let mut next = Some(format!("{}/children", self.root));
        while let Some(page) = next {
            let url = Url::parse(&page).map_err(|_| fail("OneDrive 分页链接无效"))?;
            if url.origin() != Url::parse(self.root).expect("Graph root").origin()
                || !url.username().is_empty()
                || url.password().is_some()
            {
                return Err(fail("OneDrive 分页地址无效"));
            }
            let data = metadata(c.get(url).bearer_auth(t))?;
            for file in data["value"]
                .as_array()
                .ok_or_else(|| fail("OneDrive 文件清单无效"))?
            {
                if !snapshot_name(file["name"].as_str().unwrap_or("")) || file["file"].is_null() {
                    continue;
                }
                let url = https(
                    file["@microsoft.graph.downloadUrl"]
                        .as_str()
                        .ok_or_else(|| fail("OneDrive 缺少下载链接"))?,
                    None,
                )?;
                merge(doc, &bytes(send(c.get(url))?, LIMIT)?)?;
            }
            next = data["@odata.nextLink"].as_str().map(str::to_owned);
        }
        Ok(None)
    }
    fn upload_sync_data(
        &self,
        c: &Client,
        t: &str,
        device: &str,
        doc: &Document,
        _own: Option<&str>,
    ) -> AppResult<()> {
        let path = format!("{}:/{}:/content", self.root, filename(device));
        send(
            c.put(path)
                .bearer_auth(t)
                .header("Content-Type", "application/json")
                .body(payload(doc)?),
        )?;
        Ok(())
    }
}
