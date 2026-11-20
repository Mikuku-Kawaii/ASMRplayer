use super::{AsmrOneProvider, KEYRING_ACCOUNT, KEYRING_SERVICE, OFFICIAL_API_BASES};
use crate::db::Database;
use crate::domain::{AppError, AppResult, ErrorCode};
use reqwest::{header, StatusCode};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

// Multiple providers can open the same native credential store during a restart.
static SITE_KEYRING: parking_lot::Mutex<()> = parking_lot::const_mutex(());

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct SiteCredential {
    pub(super) token: String,
    pub(super) name: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct SavedCredential {
    origin: String,
    #[serde(flatten)]
    credential: SiteCredential,
}

pub(super) struct SiteAccount {
    pub(super) base: String,
    pub(super) credential: Option<SiteCredential>,
    pub(super) revision: u64,
    legacy_base: String,
    service: String,
}

impl SiteAccount {
    pub(super) fn open(db: &Database, base: String) -> Self {
        // The old Windows entry held one raw token for the configured source.
        let legacy_base = db
            .get_setting("asmr_one_legacy_credential_base")
            .ok()
            .flatten()
            .unwrap_or_else(|| {
                let _ = db.set_setting("asmr_one_legacy_credential_base", &base);
                base.clone()
            });
        let service = if cfg!(test) {
            format!(
                "{KEYRING_SERVICE}.test.{}",
                hex::encode(Sha256::digest(db.data_dir.to_string_lossy().as_bytes()))
            )
        } else {
            KEYRING_SERVICE.to_owned()
        };
        let mut state = Self {
            base,
            credential: None,
            revision: 0,
            legacy_base,
            service,
        };
        state.credential = state.read(&state.base);
        state
    }

    fn entry(&self, base: &str) -> AppResult<keyring::Entry> {
        let key = if base == self.legacy_base {
            KEYRING_ACCOUNT.to_owned()
        } else {
            format!(
                "{KEYRING_ACCOUNT}:{}",
                hex::encode(Sha256::digest(base.as_bytes()))
            )
        };
        keyring::Entry::new(&self.service, &key).map_err(|_| Self::storage_error())
    }

    fn storage_error() -> AppError {
        AppError::new(
            ErrorCode::InternalError,
            "无法访问 Windows 凭据管理器，请重试。",
        )
    }

    fn read(&self, base: &str) -> Option<SiteCredential> {
        let _store = SITE_KEYRING.lock();
        let raw = self.entry(base).ok()?.get_password().ok()?;
        if raw.starts_with('{') {
            let saved: SavedCredential = serde_json::from_str(&raw).ok()?;
            return (saved.origin == base).then_some(saved.credential);
        }
        (base == self.legacy_base && !raw.trim().is_empty()).then_some(SiteCredential {
            token: raw,
            name: None,
        })
    }

    pub(super) fn switch(&mut self, base: String) {
        if self.base != base {
            self.credential = self.read(&base);
            self.base = base;
            self.revision += 1;
        }
    }

    pub(super) fn save(&mut self, credential: Option<SiteCredential>) -> AppResult<()> {
        let _store = SITE_KEYRING.lock();
        let entry = self.entry(&self.base)?;
        let result = match &credential {
            Some(c) => {
                let saved = SavedCredential {
                    origin: self.base.clone(),
                    credential: c.clone(),
                };
                entry.set_password(
                    &serde_json::to_string(&saved).map_err(|_| Self::storage_error())?,
                )
            }
            None => match entry.delete_credential() {
                Err(keyring::Error::NoEntry) => Ok(()),
                other => other,
            },
        };
        result.map_err(|_| Self::storage_error())?;
        self.credential = credential;
        self.revision += 1;
        Ok(())
    }
}

impl AsmrOneProvider {
    pub fn logout_site_account(&self, base: &str) -> AppResult<()> {
        let mut state = self.account.write();
        if state.base != base {
            return Err(source_changed());
        }
        state.save(None)
    }

    pub fn site_account_name(&self) -> Option<String> {
        self.account.read().credential.as_ref()?.name.clone()
    }

    pub(super) fn token_for_url(&self, address: &str) -> Option<String> {
        let state = self.account.read();
        let credential = state.credential.as_ref()?;
        let origin = url::Url::parse(&state.base).ok()?.origin();
        (url::Url::parse(address).ok()?.origin() == origin).then(|| credential.token.clone())
    }

    pub async fn login_site_account(
        &self,
        base: &str,
        name: &str,
        password: &str,
    ) -> AppResult<()> {
        let name = name.trim();
        if name.is_empty() || password.is_empty() {
            return Err(AppError::new(
                ErrorCode::InvalidInput,
                "请输入账号名和密码。",
            ));
        }
        let official = OFFICIAL_API_BASES.contains(&base);
        let test_origin = cfg!(test)
            && url::Url::parse(base)
                .is_ok_and(|url| url.scheme() == "http" && url.host_str() == Some("127.0.0.1"));
        if !official && !test_origin {
            return Err(AppError::new(
                ErrorCode::InvalidInput,
                "请选择 asmr.one 官方镜像后登录。",
            ));
        }
        let revision = {
            let state = self.account.read();
            if state.base != base {
                return Err(source_changed());
            }
            state.revision
        };
        // A password POST is never retried or followed to another URL.
        let client = self
            .api_client_builder(self.proxy_url().as_deref())?
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| AppError::new(ErrorCode::NetworkUnavailable, "无法创建站点登录连接。"))?;
        let response = client
            .post(format!("{base}/api/auth/me"))
            .header(header::ACCEPT, "application/json, text/plain, */*")
            .header(header::ORIGIN, "https://asmr.one")
            .header(header::REFERER, "https://asmr.one/")
            .json(&serde_json::json!({"name": name, "password": password}))
            .send()
            .await
            .map_err(|_| {
                AppError::new(
                    ErrorCode::NetworkUnavailable,
                    "暂时无法连接 asmr.one，请稍后重试。",
                )
            })?;
        match response.status() {
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN | StatusCode::BAD_REQUEST => {
                return Err(invalid_login())
            }
            StatusCode::TOO_MANY_REQUESTS => {
                return Err(AppError::new(
                    ErrorCode::RateLimited,
                    "登录尝试过于频繁，请稍后重试。",
                ))
            }
            status if !status.is_success() => {
                return Err(AppError::new(
                    ErrorCode::NetworkUnavailable,
                    "站点暂时无法完成登录，请稍后重试。",
                ))
            }
            _ => {}
        }
        #[derive(Deserialize)]
        struct Login {
            user: super::recommendations::RecommendationUser,
            token: String,
        }
        let login: Login = response.json().await.map_err(|_| {
            AppError::new(ErrorCode::ProviderSchemaChanged, "无法读取站点登录结果。")
        })?;
        if !login.user.logged_in
            || login.user.name.as_deref() == Some("guest")
            || login.token.trim().is_empty()
        {
            return Err(invalid_login());
        }
        let mut state = self.account.write();
        if state.base != base || state.revision != revision {
            return Err(source_changed());
        }
        state.save(Some(SiteCredential {
            token: login.token,
            name: login.user.name.or_else(|| Some(name.to_owned())),
        }))
    }
}

fn invalid_login() -> AppError {
    AppError::new(ErrorCode::AuthRequired, "账号名或密码未通过验证。")
}
fn source_changed() -> AppError {
    AppError::new(ErrorCode::SourceChanged, "站点或账号已切换，请重新登录。")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::{
        matchers::{body_json, method, path},
        Mock, MockServer, ResponseTemplate,
    };

    #[test]
    fn legacy_raw_token_remains_on_the_previously_configured_source() {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open_and_migrate(&dir.path().join("d"), &dir.path().join("c")).unwrap();
        let base = "https://api.asmr-100.com";
        db.set_setting("api_base_url", base).unwrap();
        let first = AsmrOneProvider::new(db.clone());
        {
            let account = first.account.read();
            let _store = SITE_KEYRING.lock();
            account
                .entry(base)
                .unwrap()
                .set_password("legacy-fixture")
                .unwrap();
        }
        let reopened = AsmrOneProvider::new(db.clone());
        assert_eq!(
            reopened.token_for_url(base).as_deref(),
            Some("legacy-fixture")
        );
        assert!(reopened.site_account_name().is_none());
        reopened.set_api_base("https://api.asmr.one").unwrap();
        let switched = AsmrOneProvider::new(db);
        assert!(!switched.has_auth_token());
        switched.set_api_base(base).unwrap();
        assert!(switched.has_auth_token());
        switched.logout_site_account(base).unwrap();
    }

    #[tokio::test]
    async fn login_and_logout_keep_credentials_on_the_selected_site() {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open_and_migrate(&dir.path().join("d"), &dir.path().join("c")).unwrap();
        let provider = AsmrOneProvider::new(db.clone());
        let site = MockServer::start().await;
        let other = MockServer::start().await;
        let base = site.uri();
        provider.set_api_base(&base).unwrap();
        provider.set_auth_token(Some("old-fixture".into())).unwrap();
        Mock::given(method("POST"))
            .and(path("/api/auth/me"))
            .and(body_json(
                json!({"name":"listener","password":" fixture-password "}),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                json!({"token":"test-session","user":{"loggedIn":true,"name":"listener"}}),
            ))
            .mount(&site)
            .await;
        provider
            .login_site_account(&base, " listener ", " fixture-password ")
            .await
            .unwrap();
        assert!(!site.received_requests().await.unwrap()[0]
            .headers
            .contains_key("authorization"));
        let reopened = AsmrOneProvider::new(db);
        assert_eq!(reopened.api_base(), base);
        {
            let site = reopened.account.read();
            let _store = SITE_KEYRING.lock();
            let raw = site
                .entry(&base)
                .unwrap()
                .get_password()
                .expect("saved site credential");
            let saved: SavedCredential =
                serde_json::from_str(&raw).expect("scoped credential record");
            assert_eq!(saved.origin, base);
            assert_eq!(saved.credential.name.as_deref(), Some("listener"));
        }
        assert_eq!(reopened.site_account_name().as_deref(), Some("listener"));
        assert_eq!(
            reopened
                .token_for_url(&format!("{base}/api/auth/me"))
                .as_deref(),
            Some("test-session")
        );
        assert!(reopened
            .token_for_url(&format!("{}/cover", other.uri()))
            .is_none());

        site.reset().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&site)
            .await;
        assert_eq!(
            reopened
                .login_site_account(&base, "listener", "wrong")
                .await
                .unwrap_err()
                .code,
            ErrorCode::AuthRequired
        );
        assert_eq!(site.received_requests().await.unwrap().len(), 1);
        assert_eq!(reopened.site_account_name().as_deref(), Some("listener"));
        site.reset().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(307).insert_header("Location", other.uri()))
            .mount(&site)
            .await;
        assert!(reopened
            .login_site_account(&base, "listener", "fixture")
            .await
            .is_err());
        assert!(other.received_requests().await.unwrap().is_empty());
        reopened.set_api_base(&other.uri()).unwrap();
        assert!(!reopened.has_auth_token());
        reopened.set_api_base(&base).unwrap();
        assert_eq!(reopened.site_account_name().as_deref(), Some("listener"));
        reopened.logout_site_account(&base).unwrap();
        assert!(!reopened.has_auth_token());
        assert!(reopened.site_account_name().is_none());
    }
}
