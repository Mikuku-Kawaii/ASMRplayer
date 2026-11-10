use super::{http, fail};
use crate::domain::AppResult;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::{Duration, Instant},
};

const ID: &str = include_str!("../../../../config/github-oauth-client-id.txt");
const TOKEN: &str = "https://github.com/login/oauth/access_token";
fn post(client: &Client, url: &str, fields: &[(&str, &str)]) -> AppResult<Value> {
    let res = client
        .post(url)
        .header("Accept", "application/json")
        .form(fields)
        .send()
        .map_err(|_| fail("无法连接 GitHub 授权服务"))?;
    if !res.status().is_success() {
        return Err(fail(format!(
            "GitHub 授权服务返回 HTTP {}",
            res.status().as_u16()
        )));
    }
    res.json().map_err(|_| fail("GitHub 授权响应无效"))
}
pub struct Challenge {
    pub code: String,
    device: String,
    expires: Instant,
    interval: u64,
}
pub fn begin() -> AppResult<Challenge> {
    let data = post(
        &http::client()?,
        "https://github.com/login/device/code",
        &[("client_id", ID.trim()), ("scope", "gist")],
    )?;
    let field = |name: &str| {
        data[name]
            .as_str()
            .filter(|v| !v.is_empty())
            .map(str::to_owned)
            .ok_or_else(|| fail("GitHub 未返回设备授权码"))
    };
    Ok(Challenge {
        code: field("user_code")?,
        device: field("device_code")?,
        expires: Instant::now()
            + Duration::from_secs(data["expires_in"].as_u64().unwrap_or(900).min(1800)),
        interval: data["interval"].as_u64().unwrap_or(5).clamp(1, 60),
    })
}
#[derive(Serialize, Deserialize)]
struct Grant {
    access: String,
    refresh: Option<String>,
    expires: i64,
}
fn grant(v: &Value, prior: Option<String>) -> AppResult<String> {
    if v.get("scope")
        .and_then(Value::as_str)
        .is_some_and(|s| s != "gist")
    {
        return Err(fail("GitHub 返回的权限与 gist 不一致"));
    }
    let access = v["access_token"]
        .as_str()
        .filter(|v| !v.is_empty())
        .ok_or_else(|| fail("GitHub 未返回访问令牌，请重新登录"))?;
    let token = Grant {
        access: access.into(),
        refresh: v["refresh_token"].as_str().map(str::to_owned).or(prior),
        expires: v["expires_in"].as_i64().map_or(i64::MAX, |n| {
            chrono::Utc::now().timestamp().saturating_add(n)
        }),
    };
    serde_json::to_string(&token).map_err(|_| fail("无法保存 GitHub 登录"))
}
pub fn finish(device: Challenge, cancel: &AtomicBool) -> AppResult<String> {
    let client = http::client()?;
    std::process::Command::new("rundll32.exe")
        .args([
            "url.dll,FileProtocolHandler",
            "https://github.com/login/device",
        ])
        .spawn()
        .map_err(|_| fail("无法打开系统浏览器"))?;
    let mut interval = device.interval;
    loop {
        for _ in 0..interval {
            if cancel.load(Ordering::Relaxed) {
                return Err(fail("已取消 GitHub 登录"));
            }
            if Instant::now() >= device.expires {
                return Err(fail("GitHub 授权码已过期，请重新登录"));
            }
            thread::sleep(Duration::from_secs(1));
        }
        let data = post(
            &client,
            TOKEN,
            &[
                ("client_id", ID.trim()),
                ("device_code", &device.device),
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ],
        )?;
        match data["error"].as_str() {
            Some("authorization_pending") => continue,
            Some("slow_down") => interval += 5,
            Some("access_denied") => return Err(fail("GitHub 授权已取消")),
            Some(_) => return Err(fail("GitHub 授权未完成，请重新登录")),
            None => return grant(&data, None),
        }
    }
}
pub fn access(client: &Client, stored: String) -> AppResult<(String, Option<String>)> {
    if !stored.starts_with('{') {
        return Ok((stored, None));
    }
    let old: Grant =
        serde_json::from_str(&stored).map_err(|_| fail("GitHub 凭据无效，请重新登录"))?;
    if old.expires > chrono::Utc::now().timestamp() + 60 {
        return Ok((old.access, None));
    }
    let refresh = old
        .refresh
        .ok_or_else(|| fail("GitHub 登录已过期，请重新登录"))?;
    let data = post(
        client,
        TOKEN,
        &[
            ("client_id", ID.trim()),
            ("grant_type", "refresh_token"),
            ("refresh_token", &refresh),
        ],
    )?;
    let encoded = grant(&data, Some(refresh))?;
    let latest: Grant = serde_json::from_str(&encoded).map_err(|_| fail("GitHub 刷新响应无效"))?;
    Ok((latest.access, Some(encoded)))
}
