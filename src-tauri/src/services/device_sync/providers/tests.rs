use super::*;
use wiremock::{
    matchers::{body_json, header, method, path, query_param},
    Mock, MockServer, ResponseTemplate,
};

const DEVICE: &str = "783f3c16-62be-420d-bcc4-41297873fb20";
#[tokio::test]
async fn gist_creates_secret_then_only_updates_the_own_secret_snapshot() {
    let server = MockServer::start().await;
    let own = filename(DEVICE);
    let doc = Document::default();
    let files = json!({(own.clone()):{"content":payload(&doc).unwrap()}});
    Mock::given(path("/gists"))
        .and(method("POST"))
        .and(body_json(
            json!({"description":DESCRIPTION,"public":false,"files":files}),
        ))
        .respond_with(ResponseTemplate::new(201))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path("/gists")).and(method("GET")).and(header("Authorization","Bearer fixture")).and(query_param("page","1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {"id":"aa","public":false,"description":DESCRIPTION},{"id":"bb","public":true,"description":DESCRIPTION}
        ]))).expect(1).mount(&server).await;
    Mock::given(path("/gists/aa"))
        .and(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "public":false,"description":DESCRIPTION,"files":files
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path("/gists/aa"))
        .and(method("PATCH"))
        .and(body_json(json!({"files":files})))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;
    let api = format!("{}/gists", server.uri());
    tokio::task::spawn_blocking(move || {
        let provider = GitHubGistProvider { api: &api };
        let c = http::client().unwrap();
        provider
            .upload_sync_data(&c, "fixture", DEVICE, &doc, None)
            .unwrap();
        let mut merged = Document::default();
        let id = provider
            .download_sync_data(&c, "fixture", DEVICE, &mut merged)
            .unwrap();
        assert_eq!(id.as_deref(), Some("aa"));
        provider
            .upload_sync_data(&c, "fixture", DEVICE, &merged, id.as_deref())
            .unwrap();
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn onedrive_uses_only_the_app_root_and_checks_paging_origin() {
    let server = MockServer::start().await;
    let root = "/v1.0/me/drive/special/approot";
    Mock::given(path(root))
        .and(method("GET"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path(format!("{root}/children")))
        .and(header("Authorization", "Bearer fixture"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(
                json!({"value":[],"@odata.nextLink":"https://foreign.invalid/page"}),
            ),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path(format!("{root}:/{}:/content", filename(DEVICE))))
        .and(method("PUT"))
        .and(body_json(Document::default()))
        .respond_with(ResponseTemplate::new(201))
        .expect(1)
        .mount(&server)
        .await;
    let api = format!("{}{root}", server.uri());
    tokio::task::spawn_blocking(move || {
        let provider = OneDriveProvider { root: &api };
        let c = http::client().unwrap();
        let mut doc = Document::default();
        assert!(provider
            .download_sync_data(&c, "fixture", DEVICE, &mut doc)
            .is_err());
        provider
            .upload_sync_data(&c, "fixture", DEVICE, &doc, None)
            .unwrap();
        assert!(MICROSOFT.accepts_scope("Files.ReadWrite.AppFolder"));
        assert!(MICROSOFT.accepts_scope(MICROSOFT.scope));
        assert!(!MICROSOFT.accepts_scope("Files.ReadWrite.AppFolder Files.ReadWrite.All"));
        assert!(!MICROSOFT.accepts_scope("offline_access"));
    })
    .await
    .unwrap();
}
