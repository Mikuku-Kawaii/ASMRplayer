use super::{parse_works_page_json, AsmrOneProvider, ProviderWork};
use crate::domain::{AppError, AppResult, ErrorCode, Page, RecommendationIdentity};
use chrono::Utc;
use rusqlite::params;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

#[derive(Deserialize)]
struct AccountResponse {
    user: RecommendationUser,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RecommendationUser {
    pub(super) logged_in: bool,
    pub(super) name: Option<String>,
    recommender_uuid: Option<String>,
}

impl AsmrOneProvider {
    async fn recommendation_profile(
        &self,
        base: &str,
        token: Option<&str>,
    ) -> AppResult<(String, RecommendationIdentity)> {
        if let Some(token) = token {
            let body = self
                .request_api_text(&format!("{base}/api/auth/me"), None, Some(token))
                .await?;
            let profile: AccountResponse = serde_json::from_str(&body).map_err(|_| {
                AppError::new(
                    ErrorCode::ProviderSchemaChanged,
                    "无法读取来源账号的推荐资料",
                )
            })?;
            let user = profile.user;
            if !user.logged_in || user.name.as_deref() == Some("guest") {
                return Err(AppError::new(
                    ErrorCode::AuthRequired,
                    "站点登录已失效，请重新登录 asmr.one。",
                ));
            }
            let id = user
                .recommender_uuid
                .filter(|id| !id.trim().is_empty())
                .ok_or_else(|| {
                    AppError::new(ErrorCode::ProviderSchemaChanged, "来源账号缺少推荐标识")
                })?;
            return Ok((id, RecommendationIdentity::Account { name: user.name }));
        }
        Ok((
            self.guest_recommender_uuid(base)?,
            RecommendationIdentity::Guest,
        ))
    }

    fn guest_recommender_uuid(&self, base: &str) -> AppResult<String> {
        let key = format!("asmr_one_guest_recommender_uuid:{base}");
        if let Some(id) = self.db.get_setting(&key)? {
            return Ok(id);
        }
        self.db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO app_settings (key, value_json, updated_at) VALUES (?1, ?2, ?3)
                 ON CONFLICT(key) DO NOTHING",
                params![key, Uuid::new_v4().to_string(), Utc::now().to_rfc3339()],
            )?;
            Ok(conn.query_row(
                "SELECT value_json FROM app_settings WHERE key = ?1",
                [key],
                |row| row.get(0),
            )?)
        })
    }

    pub(super) async fn recommended_works(
        &self,
        query: &str,
        page: u32,
        subtitle_only: bool,
    ) -> AppResult<Page<ProviderWork>> {
        let (base, token, revision) = {
            let state = self.account.read();
            (
                state.base.clone(),
                state.credential.as_ref().map(|c| c.token.clone()),
                state.revision,
            )
        };
        let (id, identity) = self.recommendation_profile(&base, token.as_deref()).await?;
        let page = page.max(1);
        let keyword = match (query, self.nsfw_enabled()) {
            ("", true) => " ".to_owned(),
            ("", false) => "$age:general$".to_owned(),
            (_, true) => query.to_owned(),
            (_, false) => format!("{query} $age:general$"),
        };
        let payload = json!({
            "keyword": keyword,
            "recommenderUuid": id,
            "page": page,
            "pageSize": Self::ONLINE_PAGE_SIZE,
            "subtitle": u8::from(subtitle_only),
            "localSubtitledWorks": [],
            "withPlaylistStatus": [],
        });
        let body = self
            .request_api_text(
                &format!("{base}/api/recommender/recommend-for-user"),
                Some(&payload),
                token.as_deref(),
            )
            .await?;
        if self.account.read().revision != revision {
            return Err(AppError::new(
                ErrorCode::SourceChanged,
                "推荐账号或站点已切换，请刷新作品。",
            ));
        }
        let mut result = parse_works_page_json(&body, page)?;
        result.recommendation_identity = Some(identity);
        Ok(self.apply_nsfw_filter(Self::fill_default_cover_urls(&base, result)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        db::Database,
        providers::filters::{OnlineFilter, OnlineFilterKind},
    };
    use serde_json::Value;
    use tempfile::tempdir;
    use wiremock::{
        matchers::{header, method, path},
        Mock, MockServer, ResponseTemplate,
    };

    #[tokio::test]
    async fn recommendation_pages_keep_guest_identity_filters_and_source_order() {
        let server = MockServer::start().await;
        let dir = tempdir().unwrap();
        let db = Database::open_and_migrate(&dir.path().join("data"), &dir.path().join("cache"))
            .unwrap();
        let provider = AsmrOneProvider::new(db.clone());
        provider.account.write().credential = None;
        provider.set_api_base(&server.uri()).unwrap();
        Mock::given(method("POST"))
            .and(path("/api/recommender/recommend-for-user"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "works": [
                    {"id": 123459, "title": "Forest", "age_category_string": "general"},
                    {"id": 123450, "title": "Rain", "age_category_string": "general"},
                    {"id": 123455, "title": "Hidden", "nsfw": true}
                ],
                "pagination": {"pageSize": 12, "totalCount": 16}
            })))
            .mount(&server)
            .await;
        let first = provider
            .browse_works(1, "recommend", "desc", false)
            .await
            .unwrap();
        assert_eq!(
            first.recommendation_identity,
            Some(RecommendationIdentity::Guest)
        );
        assert_eq!(
            first
                .items
                .iter()
                .map(|w| w.source_work_id.as_str())
                .collect::<Vec<_>>(),
            ["123459", "123450"]
        );
        assert!(first.items[0]
            .cover_url
            .as_ref()
            .unwrap()
            .ends_with("/api/cover/123459.jpg?type=main"));

        let reopened = AsmrOneProvider::new(db);
        reopened.account.write().credential = None;
        let filters = [OnlineFilter {
            kind: OnlineFilterKind::Tag,
            value: "雨".into(),
            exclude: false,
        }];
        let second = reopened
            .search_filtered_works("森林", &filters, 2, "recommend", "asc", true)
            .await
            .unwrap();
        assert_eq!(
            (second.page, second.page_size, second.total_count),
            (2, 12, 15)
        );
        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 2);
        let first_body: Value = requests[0].body_json().unwrap();
        let id = first_body["recommenderUuid"].as_str().unwrap();
        assert!(Uuid::parse_str(id).is_ok());
        assert_eq!(first_body["keyword"], "$age:general$");
        assert_eq!(
            requests[1].body_json::<Value>().unwrap(),
            json!({
                "keyword": "森林 $tag:雨$ $age:general$", "recommenderUuid": id,
                "page": 2, "pageSize": 12, "subtitle": 1, "localSubtitledWorks": [], "withPlaylistStatus": []
            })
        );

        server.reset().await;
        Mock::given(method("POST"))
            .and(path("/api/recommender/recommend-for-user"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        let error = reopened
            .browse_works(1, "recommend", "desc", false)
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::SourceNotFound);
        assert_eq!(server.received_requests().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn signed_in_recommendations_use_the_source_accounts_id() {
        let server = MockServer::start().await;
        let dir = tempdir().unwrap();
        let db = Database::open_and_migrate(&dir.path().join("data"), &dir.path().join("cache"))
            .unwrap();
        let provider = AsmrOneProvider::new(db);
        provider.set_api_base(&server.uri()).unwrap();
        provider.set_nsfw_enabled(true).unwrap();
        provider.account.write().credential = Some(crate::providers::SiteCredential {
            token: "fixture".to_owned(),
            name: None,
        });
        Mock::given(method("GET"))
            .and(path("/api/auth/me"))
            .and(header("Authorization", "Bearer fixture"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "user": {"loggedIn": true, "name": "test", "recommenderUuid": "account-id"}
            })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/recommender/recommend-for-user"))
            .and(header("Authorization", "Bearer fixture"))
            .and(wiremock::matchers::body_partial_json(
                json!({"recommenderUuid": "account-id", "keyword": " "}),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "works": [{"id": 123459, "title": "Quiet forest"}, {"id": 123450, "title": "Evening rain"}],
                "pagination": {"currentPage": 1, "pageSize": 12, "totalCount": 2}
            })))
            .mount(&server)
            .await;
        let page = provider
            .browse_works(1, "recommend", "desc", false)
            .await
            .unwrap();
        assert_eq!(
            page.items
                .iter()
                .map(|w| w.source_work_id.as_str())
                .collect::<Vec<_>>(),
            ["123459", "123450"]
        );
        assert_eq!(
            serde_json::to_value(&page).unwrap()["recommendationIdentity"],
            json!({"kind": "account", "name": "test"})
        );
        assert_eq!(server.received_requests().await.unwrap().len(), 2);

        Mock::given(method("POST"))
            .and(path("/api/recommender/popular"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "works": [{"id": 123456, "title": "Popular work"}],
                "pagination": {"currentPage": 1, "pageSize": 20, "totalCount": 1}
            })))
            .mount(&server)
            .await;
        let popular = provider
            .browse_works(1, "popular", "desc", false)
            .await
            .unwrap();
        assert_eq!(popular.items[0].source_work_id, "123456");
        assert!(popular.recommendation_identity.is_none());

        for user in [
            json!({"loggedIn": false}),
            json!({"loggedIn": true, "name": "guest", "recommenderUuid": "guest-id"}),
        ] {
            server.reset().await;
            Mock::given(method("GET"))
                .and(path("/api/auth/me"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({"user": user})))
                .mount(&server)
                .await;
            let error = provider
                .browse_works(1, "recommend", "desc", false)
                .await
                .unwrap_err();
            assert_eq!(error.code, ErrorCode::AuthRequired);
            assert_eq!(server.received_requests().await.unwrap().len(), 1);
        }
    }
}
