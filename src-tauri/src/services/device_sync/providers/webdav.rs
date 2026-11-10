use super::{
    bytes, http, fail, merge, payload, save, AppResult, Client, Document, OAuth, SyncProvider,
    Url, Value, LIMIT,
};
use serde::{Deserialize, Serialize};

const FILE: &str = "asmr-player-sync-v1.json";

#[derive(Serialize, Deserialize)]
struct Account {
    url: String,
    username: String,
    password: String,
}
impl Account {
    fn file(&self) -> AppResult<Url> {
        let mut base =
            Url::parse(self.url.trim()).map_err(|_| fail("请输入 WebDAV HTTPS 文件夹地址"))?;
        if base.scheme() != "https"
            || base.host_str().is_none()
            || !base.username().is_empty()
            || base.password().is_some()
            || base.query().is_some()
            || base.fragment().is_some()
        {
            return Err(fail("WebDAV 地址需要使用 HTTPS，账号和密码请分别填写"));
        }
        if self.username.is_empty() || self.username.contains(':') || self.password.is_empty() {
            return Err(fail("请填写 WebDAV 用户名和密码"));
        }
        base.path_segments_mut()
            .map_err(|_| fail("无效的 WebDAV 文件夹"))?
            .pop_if_empty()
            .push(FILE);
        Ok(base)
    }
}

pub(super) struct WebDavProvider;
fn fetch(
    client: &Client,
    account: &Account,
    url: &Url,
    doc: &mut Document,
) -> AppResult<Option<String>> {
    let response = client
        .get(url.clone())
        .basic_auth(&account.username, Some(&account.password))
        .header("Cache-Control", "no-cache")
        .send()
        .map_err(|_| fail("无法连接 WebDAV，请检查地址和网络"))?;
    if response.status().as_u16() == 404 {
        return Ok(None);
    }
    if response.status().as_u16() != 200 {
        return Err(fail(format!(
            "WebDAV 读取失败（HTTP {}）",
            response.status().as_u16()
        )));
    }
    let etag = response
        .headers()
        .get("ETag")
        .and_then(|v| v.to_str().ok())
        .filter(|v| v.len() >= 2 && v.starts_with('"') && v.ends_with('"'))
        .ok_or_else(|| fail("WebDAV 服务需要为同步文件返回强 ETag"))?
        .to_owned();
    merge(doc, &bytes(response, LIMIT)?)?;
    Ok(Some(etag))
}
fn put(
    client: &Client,
    account: &Account,
    url: &Url,
    doc: &Document,
    revision: Option<&str>,
) -> AppResult<bool> {
    let (header, value) = revision.map_or(("If-None-Match", "*"), |tag| ("If-Match", tag));
    let r = client
        .put(url.clone())
        .basic_auth(&account.username, Some(&account.password))
        .header(header, value)
        .header("Content-Type", "application/json")
        .body(payload(doc)?)
        .send()
        .map_err(|_| fail("无法写入 WebDAV，请检查网络"))?;
    if r.status().as_u16() == 412 {
        return Ok(false);
    }
    if !r.status().is_success() {
        return Err(fail(format!(
            "WebDAV 写入失败（HTTP {}），请确认文件夹存在且可写",
            r.status().as_u16()
        )));
    }
    Ok(true)
}
fn account(value: &str) -> AppResult<Account> {
    serde_json::from_str(value).map_err(|_| fail("请重新绑定 WebDAV"))
}
impl SyncProvider for WebDavProvider {
    fn oauth(&self) -> Option<&OAuth> {
        None
    }
    fn login(&self, entry: &keyring::Entry, input: &Value) -> AppResult<()> {
        let a: Account = serde_json::from_value(input.clone())
            .map_err(|_| fail("请填写 WebDAV 地址、用户名和密码"))?;
        fetch(&http::client()?, &a, &a.file()?, &mut Document::default())?;
        save(
            entry,
            &serde_json::to_string(&a).map_err(|_| fail("无法保存 WebDAV 配置"))?,
        )
    }
    fn download_sync_data(
        &self,
        c: &Client,
        t: &str,
        _: &str,
        doc: &mut Document,
    ) -> AppResult<Option<String>> {
        let a = account(t)?;
        fetch(c, &a, &a.file()?, doc)
    }
    fn upload_sync_data(
        &self,
        c: &Client,
        t: &str,
        _: &str,
        doc: &Document,
        rev: Option<&str>,
    ) -> AppResult<()> {
        let a = account(t)?;
        if !put(c, &a, &a.file()?, doc, rev)? {
            return Err(fail("远端资料已更新，请重试"));
        }
        Ok(())
    }
    fn exchange_sync_data(
        &self,
        c: &Client,
        t: &str,
        _: &str,
        doc: &mut Document,
    ) -> AppResult<()> {
        let a = account(t)?;
        exchange(c, &a, &a.file()?, doc)
    }
}
fn exchange(c: &Client, a: &Account, url: &Url, doc: &mut Document) -> AppResult<()> {
    for _ in 0..3 {
        let rev = fetch(c, a, url, doc)?;
        if put(c, a, url, doc, rev.as_deref())? {
            return Ok(());
        }
    }
    Err(fail("其他设备正在更新 WebDAV 文件，请稍后同步"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::{
        matchers::{header, method, path},
        Mock, MockServer, ResponseTemplate,
    };
    #[tokio::test]
    async fn concurrent_create_reloads_and_updates_with_matching_etag() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/sync"))
            .respond_with(ResponseTemplate::new(404))
            .up_to_n_times(1)
            .expect(1)
            .with_priority(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/sync"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("etag", "\"new\"")
                    .set_body_json(Document::default()),
            )
            .expect(1)
            .with_priority(2)
            .mount(&server)
            .await;
        Mock::given(method("PUT"))
            .and(header("If-None-Match", "*"))
            .respond_with(ResponseTemplate::new(412))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("PUT"))
            .and(header("If-Match", "\"new\""))
            .respond_with(ResponseTemplate::new(204))
            .expect(1)
            .mount(&server)
            .await;
        let url = Url::parse(&format!("{}/sync", server.uri())).unwrap();
        tokio::task::spawn_blocking(move || {
            exchange(
                &http::client().unwrap(),
                &Account {
                    url: "https://example.org/dav".into(),
                    username: "fixture".into(),
                    password: "fixture".into(),
                },
                &url,
                &mut Document::default(),
            )
        })
        .await
        .unwrap()
        .unwrap();
    }
}
