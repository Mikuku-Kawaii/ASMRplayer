use super::super::http::client as http;
use super::*;
use serde_json::json;
use wiremock::{
    matchers::{body_string_contains, method, path},
    Mock, MockServer, ResponseTemplate,
};
#[tokio::test]
async fn microsoft_callback_validates_state_and_pkce() {
    verify_callback(&MICROSOFT, "login.microsoftonline.com", "localhost").await
}
async fn verify_callback(
    provider: &'static OAuth,
    host: &'static str,
    callback_host: &'static str,
) {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .and(body_string_contains("code=correct-code"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "scope": provider.scope, "refresh_token": "test-refresh"
        })))
        .expect(1)
        .mount(&server)
        .await;
    let token_url = format!("{}/token", server.uri());
    let (challenge, redirect) = tokio::task::spawn_blocking(move || {
        let mut challenge = String::new();
        let mut redirect = String::new();
        let token = authorize(provider, "test-client", &token_url, |url| {
            let query: std::collections::HashMap<_, _> = url.query_pairs().collect();
            assert_eq!(url.host_str(), Some(host));
            assert_eq!(query["code_challenge_method"], "S256");
            assert_eq!(query["client_id"], "test-client");
            assert_eq!(query["scope"], provider.scope);
            assert!(!query.contains_key("code_verifier"));
            challenge = query["code_challenge"].to_string();
            redirect = query["redirect_uri"].to_string();
            let callback = Url::parse(&redirect).unwrap();
            assert_eq!(callback.host_str(), Some(callback_host));
            let port = callback.port().unwrap();
            let state = query["state"].to_string();
            std::thread::spawn(move || {
                for (state, code) in [
                    ("incorrect", "invalid-code"),
                    (state.as_str(), "correct-code"),
                ] {
                    let mut conn = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
                    write!(
                        conn,
                        "GET /?state={state}&code={code} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n"
                    )
                    .unwrap();
                }
            });
            Ok(())
        })
        .unwrap();
        assert_eq!(token, "test-refresh");
        (challenge, redirect)
    })
    .await
    .unwrap();
    let requests = server.received_requests().await.unwrap();
    let fields: std::collections::HashMap<_, _> =
        url::form_urlencoded::parse(&requests[0].body).collect();
    assert_eq!(fields["client_id"], "test-client");
    assert_eq!(fields["redirect_uri"], redirect);
    assert_eq!(fields["grant_type"], "authorization_code");
    assert!(!fields.contains_key("client_secret"));
    let verifier = &fields["code_verifier"];
    assert_eq!(verifier.len(), 43);
    assert_eq!(
        URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes())),
        challenge
    );
}

#[tokio::test]
async fn refresh_rejects_extra_drive_permissions_and_accepts_scope_omission() {
    let server = MockServer::start().await;
    for (path, grant) in [
        (
            "/wide",
            json!({"access_token":"fixture","scope":format!("{} email",MICROSOFT.scope)}),
        ),
        (
            "/same",
            json!({"access_token":"fixture","refresh_token":"rotated"}),
        ),
    ] {
        Mock::given(method("POST"))
            .and(wiremock::matchers::path(path))
            .and(body_string_contains("grant_type=refresh_token"))
            .and(body_string_contains("refresh_token=original"))
            .respond_with(ResponseTemplate::new(200).set_body_json(grant))
            .expect(1)
            .mount(&server)
            .await;
    }
    let base = server.uri();
    tokio::task::spawn_blocking(move || {
        let client = http().unwrap();
        let refresh = |path: &str| {
            token(
                &MICROSOFT,
                &client,
                "test",
                &format!("{base}{path}"),
                vec![
                    ("grant_type", "refresh_token"),
                    ("refresh_token", "original"),
                ],
            )
        };
        assert!(refresh("/wide").is_err());
        assert_eq!(refresh("/same").unwrap()["refresh_token"], "rotated");
    })
    .await
    .unwrap();
}
