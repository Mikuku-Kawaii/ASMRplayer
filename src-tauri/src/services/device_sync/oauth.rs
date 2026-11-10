use super::fail;
use crate::domain::AppResult;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use reqwest::blocking::Client;
use ring::rand::{SecureRandom, SystemRandom};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    time::{Duration, Instant},
};
use url::Url;

pub struct OAuth {
    id: Option<&'static str>,
    token_url: &'static str,
    authorize_url: &'static str,
    pub scope: &'static str,
    host: &'static str,
    prompt: &'static str,
}
pub const MICROSOFT: OAuth = OAuth {
    id: match option_env!("ASMR_ONEDRIVE_CLIENT_ID") {
        Some(id) if !id.is_empty() => Some(id),
        _ => Some(include_str!("../../../../config/onedrive-oauth-client-id.txt")),
    },
    token_url: "https://login.microsoftonline.com/common/oauth2/v2.0/token",
    authorize_url: "https://login.microsoftonline.com/common/oauth2/v2.0/authorize",
    scope: "Files.ReadWrite.AppFolder offline_access",
    host: "localhost",
    prompt: "select_account",
};
impl OAuth {
    pub fn id(&self) -> AppResult<&str> {
        self.id
            .map(str::trim)
            .filter(|s| uuid::Uuid::parse_str(s).is_ok())
            .ok_or_else(|| fail("此版本暂未启用此存储的登录"))
    }
    pub fn accepts_scope(&self, value: &str) -> bool {
        let mut folder = false;
        for scope in value.split_whitespace() {
            let scope = scope
                .strip_prefix("https://graph.microsoft.com/")
                .unwrap_or(scope);
            if scope.eq_ignore_ascii_case("Files.ReadWrite.AppFolder") {
                folder = true
            } else if scope != "offline_access" {
                return false;
            }
        }
        folder
    }
}
fn random() -> AppResult<String> {
    let mut bytes = [0; 32];
    SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| fail("无法生成授权随机数"))?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}
pub(super) fn token<'a>(
    provider: &OAuth,
    client: &Client,
    client_id: &'a str,
    endpoint: &str,
    mut params: Vec<(&str, &'a str)>,
) -> AppResult<Value> {
    params.push(("client_id", client_id));
    params.push(("scope", provider.scope));
    let response = client
        .post(endpoint)
        .form(&params)
        .send()
        .map_err(|_| fail("无法连接账号授权服务"))?;
    if !response.status().is_success() {
        return Err(fail("账号授权未成功，请检查客户端配置或重新登录"));
    }
    let grant: Value = response.json().map_err(|_| fail("账号授权响应无效"))?;
    // OAuth permits omitting scope when it is identical to the requested grant.
    if grant
        .get("scope")
        .is_some_and(|v| !provider.accepts_scope(v.as_str().unwrap_or("")))
    {
        return Err(fail("返回的权限超出应用文件夹范围"));
    }
    Ok(grant)
}
pub fn access_token(
    provider: &OAuth,
    client: &Client,
    refresh: &str,
) -> AppResult<(String, Option<String>)> {
    let v = token(
        provider,
        client,
        provider.id()?,
        provider.token_url,
        vec![("grant_type", "refresh_token"), ("refresh_token", refresh)],
    )?;
    let access = v["access_token"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| fail("授权服务未返回访问令牌"))?;
    Ok((
        access.to_owned(),
        v["refresh_token"]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(str::to_owned),
    ))
}
pub fn login(provider: &OAuth) -> AppResult<String> {
    authorize(provider, provider.id()?, provider.token_url, |url| {
        std::process::Command::new("rundll32.exe")
            .args(["url.dll,FileProtocolHandler", url.as_str()])
            .spawn()
            .map_err(|_| fail("无法打开系统浏览器"))?;
        Ok(())
    })
}

pub(super) fn authorize(
    provider: &OAuth,
    client_id: &str,
    token_url: &str,
    open_browser: impl FnOnce(&Url) -> AppResult<()>,
) -> AppResult<String> {
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|_| fail("无法创建本机授权回调"))?;
    listener.set_nonblocking(true)?;
    let redirect = format!(
        "http://{}:{}/",
        provider.host,
        listener.local_addr()?.port()
    );
    let verifier = random()?;
    let state = random()?;
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let mut url = Url::parse(provider.authorize_url).unwrap();
    url.query_pairs_mut().extend_pairs([
        ("client_id", client_id),
        ("redirect_uri", &redirect),
        ("response_type", "code"),
        ("scope", provider.scope),
        ("state", &state),
        ("code_challenge", &challenge),
        ("code_challenge_method", "S256"),
        ("prompt", provider.prompt),
    ]);
    open_browser(&url)?;
    let end = Instant::now() + Duration::from_secs(180);
    loop {
        if Instant::now() >= end {
            return Err(fail("账号登录已超时，请重新登录"));
        }
        let (mut socket, _) = match listener.accept() {
            Ok(v) => v,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(100));
                continue;
            }
            Err(e) => return Err(e.into()),
        };
        socket.set_nonblocking(false)?;
        socket.set_read_timeout(Some(Duration::from_secs(3)))?;
        let mut line = String::new();
        if BufReader::new((&socket).take(8192))
            .read_line(&mut line)
            .is_err()
        {
            continue;
        }
        let mut fields = line.split_whitespace();
        if fields.next() != Some("GET") {
            continue;
        }
        let Some(path) = fields.next().filter(|p| p.starts_with("/?")) else {
            continue;
        };
        let Ok(callback) = Url::parse(&format!("http://localhost{path}")) else {
            continue;
        };
        let query: std::collections::HashMap<_, _> = callback.query_pairs().collect();
        if query.get("state").map(|v| v.as_ref()) != Some(state.as_str()) {
            continue;
        }
        let response = "HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n请返回 ASMR Player 查看登录结果。";
        let _ = socket.write_all(response.as_bytes());
        drop(socket);
        let code = query
            .get("code")
            .ok_or_else(|| fail("账号登录已取消或授权被拒绝"))?;
        let v = token(
            provider,
            &super::http::client()?,
            client_id,
            token_url,
            vec![
                ("grant_type", "authorization_code"),
                ("code", code),
                ("redirect_uri", &redirect),
                ("code_verifier", &verifier),
            ],
        )?;
        return v["refresh_token"]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .ok_or_else(|| fail("授权服务未返回离线授权，请重新登录"));
    }
}

#[cfg(test)]
mod tests;
