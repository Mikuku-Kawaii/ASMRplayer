use super::*;
use crate::db::repo::imported_work_code;
use crate::services::library::extract_work_code;

pub(super) struct WorkLookup {
    pub id: String,
    store_code: Option<String>,
}

impl WorkLookup {
    pub fn matches_code(&self, actual: Option<&str>) -> bool {
        self.store_code
            .as_deref()
            .is_none_or(|expected| actual == Some(expected))
    }
}

impl AsmrOneProvider {
    pub(super) fn library_work_input(
        &self,
        work_id: &str,
        input: Option<&str>,
    ) -> AppResult<String> {
        if let Some(input) = input.map(str::trim).filter(|s| !s.is_empty()) {
            return Ok(input.to_owned());
        }
        self.db.with_conn(|conn| {
            let (source, code, title): (Option<String>, Option<String>, String) = conn.query_row(
                "SELECT (SELECT source_work_id FROM work_sources WHERE work_id=w.id AND provider=?2
                         ORDER BY refreshed_at DESC LIMIT 1), NULLIF(w.rj_code,''), w.title FROM works w WHERE w.id=?1",
                params![work_id, PROVIDER_ASMR_ONE], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            ).optional()?.ok_or_else(|| AppError::new(ErrorCode::SourceNotFound, "未找到该作品"))?;
            if let Some(input) = source.or(code) {
                return Ok(input);
            }
            imported_work_code(conn, work_id, &title)?.ok_or_else(|| AppError::new(
                ErrorCode::InvalidInput, "未识别到作品编号，请在详情页输入 RJ、VJ、BJ 编号或站内 ID",
            ))
        })
    }

    pub(super) async fn resolve_work_input(&self, raw: &str) -> AppResult<WorkLookup> {
        let code = raw.trim().to_ascii_uppercase();
        if !code.starts_with("VJ") && !code.starts_with("BJ") {
            return Ok(WorkLookup {
                id: normalize_work_id_input(raw)?,
                store_code: None,
            });
        }
        if extract_work_code(&code).as_deref() != Some(&code)
            || code[2..].bytes().all(|b| b == b'0')
        {
            return Err(AppError::new(ErrorCode::InvalidInput, "无效的作品编号"));
        }

        // VJ/BJ numbers belong to the store. Only the exact search hit supplies the site's ID.
        let base = self.api_base();
        let mut page = 1;
        loop {
            let body = self
                .get_text_with_retry(&format!("{base}/api/search/{code}?page={page}"))
                .await?;
            let result = parse_works_page_json(&body, page)?;
            if let Some(work) = result
                .items
                .iter()
                .find(|w| w.rj_code.as_deref() == Some(&code))
            {
                return Ok(WorkLookup {
                    id: normalize_work_id_input(&work.source_work_id)?,
                    store_code: Some(code),
                });
            }
            if result.items.is_empty()
                || result.page_size == 0
                || result.page != page
                || u64::from(page) * u64::from(result.page_size) >= result.total_count
            {
                return Err(AppError::new(
                    ErrorCode::SourceNotFound,
                    format!("来源未找到编号为 {code} 的作品"),
                ));
            }
            page += 1;
        }
    }

    pub(super) async fn fetch_work_info(&self, lookup: &WorkLookup) -> AppResult<ProviderWork> {
        let body = self.work_info_body(&lookup.id).await?;
        let work = parse_work_info_json(&body)?;
        if work.source_work_id != lookup.id || !lookup.matches_code(work.rj_code.as_deref()) {
            return Err(AppError::new(
                ErrorCode::ProviderSchemaChanged,
                "来源返回的作品身份不一致",
            ));
        }
        Ok(work)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::library::LibraryService;
    use serde_json::json;
    use tempfile::tempdir;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn local_vj_import_and_legacy_sync_use_the_exact_store_code() {
        let server = MockServer::start().await;
        let temp = tempdir().unwrap();
        let db = Database::open_and_migrate(&temp.path().join("db"), &temp.path().join("cache"))
            .unwrap();
        let provider = AsmrOneProvider::new(db.clone());
        provider.set_api_base(&server.uri()).unwrap();
        let library = LibraryService::new(db.clone());
        let folder = temp.path().join("旧库 [vj01002150]");
        std::fs::create_dir(&folder).unwrap();
        std::fs::write(
            folder.join("01.mp3"),
            include_bytes!("../../../tests/fixtures/audio/stereo_lr_check.mp3"),
        )
        .unwrap();
        let folder = folder.to_str().unwrap();
        let preview = library.preview_import(folder, "single_work").unwrap();
        assert_eq!(
            preview.candidates[0].detected_rj_code.as_deref(),
            Some("VJ01002150")
        );
        library
            .scan_and_import(folder, "single_work", None)
            .unwrap();
        let work = db.list_works("all", None).unwrap().pop().unwrap();
        assert_eq!(work.rj_code.as_deref(), Some("VJ01002150"));
        let original = db.get_work_detail(&work.id).unwrap();
        // An older scan saved only the directory and media paths. Even a renamed title can be repaired.
        db.with_conn(|conn| {
            conn.execute(
                "UPDATE works SET rj_code=NULL, title='本地标题' WHERE id=?1",
                [&work.id],
            )?;
            Ok(())
        })
        .unwrap();
        assert_eq!(
            db.get_work_detail(&work.id).unwrap().rj_code.as_deref(),
            Some("VJ01002150")
        );

        let other = json!({"id":1002150, "source_id":"RJ01002150", "title":"VJ01002150"});
        let exact =
            json!({"id":100000041, "source_id":"VJ01002150", "title":"来源标题", "name":"社团"});
        for (page, item) in [(1, other), (2, exact.clone())] {
            Mock::given(method("GET"))
                .and(path("/api/search/VJ01002150"))
                .and(query_param("page", page.to_string()))
                .respond_with(
                    ResponseTemplate::new(200).set_body_json(json!({"works":[item],
                    "pagination":{"currentPage":page,"pageSize":1,"totalCount":2}})),
                )
                .expect(1)
                .mount(&server)
                .await;
        }
        Mock::given(method("GET"))
            .and(path("/api/workInfo/100000041"))
            .respond_with(ResponseTemplate::new(200).set_body_json(exact))
            .expect(1)
            .mount(&server)
            .await;
        let synced = provider
            .sync_work_from_asmr_one(&work.id, None, false)
            .await
            .unwrap();
        assert_eq!(synced.title, "来源标题");
        assert_eq!(synced.circle_name, "社团");
        assert_eq!(synced.rj_code.as_deref(), Some("VJ01002150"));
        assert_eq!(
            synced.tracks[0].versions[0].id,
            original.tracks[0].versions[0].id
        );
        assert!(server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .all(|r| !r.url.path().starts_with("/api/tracks/")));
    }

    #[tokio::test]
    async fn local_sync_rejects_search_or_detail_for_a_different_code() {
        let server = MockServer::start().await;
        let temp = tempdir().unwrap();
        let db = Database::open_and_migrate(&temp.path().join("db"), &temp.path().join("cache"))
            .unwrap();
        let provider = AsmrOneProvider::new(db.clone());
        provider.set_api_base(&server.uri()).unwrap();
        db.with_conn(|conn| {
            conn.execute("INSERT INTO works(id,title,created_at,updated_at) VALUES('local','原始标题','now','now')", [])?;
            Ok(())
        }).unwrap();
        let other = json!({"id":1002150, "source_id":"RJ01002150", "title":"VJ01002150"});
        Mock::given(method("GET"))
            .and(path("/api/search/VJ01002150"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"works":[other]})))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/search/BJ01002150"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                json!({"works":[{"id":100000041,"source_id":"BJ01002150","title":"样本"}]}),
            ))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/workInfo/100000041"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(
                    json!({"id":100000041,"source_id":"VJ01002150","title":"其他作品"}),
                ),
            )
            .expect(1)
            .mount(&server)
            .await;
        for (input, error) in [
            ("vj01002150", ErrorCode::SourceNotFound),
            (" bj01002150 ", ErrorCode::ProviderSchemaChanged),
            ("作品号", ErrorCode::InvalidInput),
            ("VJ00000000", ErrorCode::InvalidInput),
        ] {
            assert_eq!(
                provider
                    .sync_work_from_asmr_one("local", Some(input), true)
                    .await
                    .unwrap_err()
                    .code,
                error
            );
        }
        assert_eq!(db.get_work_detail("local").unwrap().title, "原始标题");
        assert!(extract_work_code("VJ010021501").is_none());
    }
}
