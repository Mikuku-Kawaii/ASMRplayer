use crate::domain::{AppError, AppResult, ErrorCode};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum OnlineFilterKind {
    VoiceActor,
    Circle,
    Tag,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OnlineFilter {
    pub kind: OnlineFilterKind,
    pub value: String,
    pub exclude: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderTag {
    pub id: String,
    pub name: String,
    pub label: String,
    pub count: u64,
}

pub fn parse_tags(body: &str) -> AppResult<Vec<ProviderTag>> {
    let value: serde_json::Value = serde_json::from_str(body)
        .map_err(|_| AppError::new(ErrorCode::ProviderSchemaChanged, "标签列表解析失败"))?;
    let items = value.as_array()
        .ok_or_else(|| AppError::new(ErrorCode::ProviderSchemaChanged, "标签列表格式已变化"))?;
    let mut tags = Vec::new();
    for item in items {
        let Some(name) = item.get("name").and_then(|v| v.as_str()).filter(|s| !s.trim().is_empty()) else { continue; };
        let Some(count) = item.get("count").and_then(|v| v.as_u64()) else { continue; };
        let Some(id) = item.get("id").and_then(|v| v.as_u64().map(|n| n.to_string()).or_else(|| v.as_str().map(str::to_owned))) else { continue; };
        let label = item.pointer("/i18n/zh-cn/name").and_then(|v| v.as_str()).filter(|s| !s.trim().is_empty()).unwrap_or(name);
        if tags.iter().any(|tag: &ProviderTag| tag.name == name) { continue; }
        tags.push(ProviderTag { id, name: name.to_owned(), label: label.to_owned(), count });
    }
    if !items.is_empty() && tags.is_empty() {
        return Err(AppError::new(ErrorCode::ProviderSchemaChanged, "标签列表缺少名称或作品数量"));
    }
    tags.sort_by(|a, b| b.count.cmp(&a.count).then(a.name.cmp(&b.name)));
    Ok(tags)
}

/// Each upstream term is ANDed, including multiple terms of the same kind.
/// Keep the query language here so renderer inputs cannot inject operators.
pub fn build_filter_query(keyword: &str, filters: &[OnlineFilter]) -> AppResult<String> {
    let invalid = || {
        AppError::new(ErrorCode::InvalidInput, "筛选内容无效，请检查名称和条件数量。")
        .with_hint("最多选择 5 个标签；名称不能包含 $ 或控制字符，关键词最多 200 字，条件名称最多 100 字。")
    };
    let keyword = keyword.trim();
    if filters.len() > 12
        || filters.iter().filter(|filter| filter.kind == OnlineFilterKind::Tag).count() > 5
        || keyword.chars().count() > 200
        || keyword.chars().any(|c| c == '$' || c.is_control())
    {
        return Err(invalid());
    }
    let mut terms = vec![keyword.to_owned()];
    for filter in filters {
        let value = filter.value.trim();
        if value.is_empty()
            || value.chars().count() > 100
            || value.chars().any(|c| c == '$' || c.is_control())
        {
            return Err(invalid());
        }
        let namespace = match filter.kind {
            OnlineFilterKind::VoiceActor => "va",
            OnlineFilterKind::Circle => "circle",
            OnlineFilterKind::Tag => "tag",
        };
        let sign = if filter.exclude { "-" } else { "" };
        let term = format!("${sign}{namespace}:{value}$");
        if !terms.contains(&term) {
            terms.push(term);
        }
    }
    Ok(terms
        .into_iter()
        .filter(|term| !term.is_empty())
        .collect::<Vec<_>>()
        .join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{db::Database, providers::AsmrOneProvider};
    use tempfile::tempdir;
    use wiremock::matchers::{method, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn tags_use_real_counts_and_translations_and_enforce_five_selections() {
        let tags = parse_tags(r#"[{"id":1,"name":"A","count":4,"i18n":{"zh-cn":{"name":"标签甲"}}},{"id":2,"name":"B","count":10},{"id":3,"name":"bad"}]"#).unwrap();
        assert_eq!(tags.len(), 2);
        assert_eq!(tags[0].name, "B");
        assert_eq!(tags[1].label, "标签甲");
        assert_eq!(tags[1].count, 4);
        assert!(parse_tags(r#"{"tags":[]}"#).is_err());
        let filters: Vec<_> = (0..6).map(|n| OnlineFilter { kind: OnlineFilterKind::Tag, value: n.to_string(), exclude: false }).collect();
        assert!(build_filter_query("", &filters[..5]).is_ok());
        assert!(build_filter_query("", &filters).is_err());
    }

    #[test]
    fn builds_and_query_with_includes_excludes_and_unicode() {
        let filters = vec![
            OnlineFilter {
                kind: OnlineFilterKind::VoiceActor,
                value: " 声優 A ".into(),
                exclude: false,
            },
            OnlineFilter {
                kind: OnlineFilterKind::Circle,
                value: "社团".into(),
                exclude: false,
            },
            OnlineFilter {
                kind: OnlineFilterKind::Tag,
                value: "音楽".into(),
                exclude: true,
            },
        ];
        assert_eq!(
            build_filter_query(" 耳かき ", &filters).unwrap(),
            "耳かき $va:声優 A$ $circle:社团$ $-tag:音楽$"
        );
    }

    #[test]
    fn rejects_operator_injection_and_excessive_input() {
        for value in ["", "foo$ $age:adult$", "foo\nbar"] {
            let filter = OnlineFilter {
                kind: OnlineFilterKind::Tag,
                value: value.into(),
                exclude: false,
            };
            assert!(build_filter_query("", &[filter]).is_err());
        }
        assert!(build_filter_query("$age:adult$", &[]).is_err());
        assert!(build_filter_query(&"字".repeat(201), &[]).is_err());
        let filter = OnlineFilter {
            kind: OnlineFilterKind::Tag,
            value: "耳かき".into(),
            exclude: false,
        };
        assert!(build_filter_query("", &vec![filter; 13]).is_err());
    }

    #[tokio::test]
    async fn sends_combined_filters_on_every_page_and_propagates_page_failure() {
        let server = MockServer::start().await;
        let dir = tempdir().unwrap();
        let db = Database::open_and_migrate(&dir.path().join("d"), &dir.path().join("c")).unwrap();
        let provider = AsmrOneProvider::new(db);
        provider.set_api_base(&server.uri()).unwrap();
        for page in 1..=2 {
            let works: Vec<_> = ((page - 1) * 20 + 1..=page * 20).map(|id| serde_json::json!({
                "id": id, "title": "合成测试作品", "nsfw": false, "vas": [], "tags": [], "circle": {"name": "社团"}
            })).collect();
            Mock::given(method("GET")).and(query_param("page", page.to_string()))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "works": works, "pagination": {"currentPage": page, "pageSize": 20, "totalCount": 40}
                }))).mount(&server).await;
        }
        let filters = vec![OnlineFilter {
            kind: OnlineFilterKind::Tag,
            value: "耳かき".into(),
            exclude: true,
        }];
        let result = provider
            .search_filtered_works("睡眠", &filters, 2, "release", "asc", true)
            .await
            .unwrap();
        assert_eq!(result.items.len(), 12);
        assert_eq!(result.items.first().unwrap().source_work_id, "13");
        assert_eq!(result.items.last().unwrap().source_work_id, "24");
        assert_eq!(result.total_count, 40);
        for request in server.received_requests().await.unwrap() {
            assert_eq!(
                urlencoding::decode(request.url.path()).unwrap(),
                "/api/search/睡眠 $-tag:耳かき$ $age:general$"
            );
            let query: std::collections::HashMap<_, _> = request.url.query_pairs().collect();
            assert_eq!(query.get("order").unwrap(), "release");
            assert_eq!(query.get("sort").unwrap(), "asc");
            assert_eq!(query.get("subtitle").unwrap(), "1");
        }
        Mock::given(method("GET"))
            .and(query_param("page", "2"))
            .respond_with(ResponseTemplate::new(404))
            .with_priority(1)
            .mount(&server)
            .await;
        assert!(provider
            .search_filtered_works("睡眠", &filters, 2, "release", "asc", true)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn empty_result_and_error_never_broaden_the_query() {
        let server = MockServer::start().await;
        let dir = tempdir().unwrap();
        let db = Database::open_and_migrate(&dir.path().join("d"), &dir.path().join("c")).unwrap();
        let provider = AsmrOneProvider::new(db);
        provider.set_api_base(&server.uri()).unwrap();
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "works": [], "pagination": {"currentPage": 1, "pageSize": 20, "totalCount": 0}
            })))
            .mount(&server)
            .await;
        assert!(provider
            .search_filtered_works("无结果", &[], 1, "popular", "desc", false)
            .await
            .unwrap()
            .items
            .is_empty());
        assert_eq!(server.received_requests().await.unwrap().len(), 1);
        server.reset().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;
        assert!(provider
            .search_filtered_works("无权限", &[], 1, "release", "desc", false)
            .await
            .is_err());
        assert_eq!(server.received_requests().await.unwrap().len(), 1);
    }
}
