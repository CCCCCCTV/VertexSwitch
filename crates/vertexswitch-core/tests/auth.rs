//! 服务账号认证（SPEC §2）：用测试时现生成的 RSA 密钥和 wiremock 假 token 端点，不连 Google。

use std::path::PathBuf;

use jsonwebtoken::{Algorithm, DecodingKey, Validation};
use rsa::pkcs8::{EncodePrivateKey, EncodePublicKey, LineEnding};
use serde_json::{Value, json};
use vertexswitch_core::auth::{SCOPE, ServiceAccountAuth};
use wiremock::matchers::{body_string_contains, method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

struct Fixture {
    _dir: tempfile::TempDir,
    key_file: PathBuf,
    public_pem: String,
    token_uri: String,
}

async fn fixture(server: &MockServer) -> Fixture {
    let private = rsa::RsaPrivateKey::new(&mut rsa::rand_core::OsRng, 2048).unwrap();
    let private_pem = private.to_pkcs8_pem(LineEnding::LF).unwrap().to_string();
    let public_pem = private
        .to_public_key()
        .to_public_key_pem(LineEnding::LF)
        .unwrap();
    let token_uri = format!("{}/token", server.uri());
    let dir = tempfile::tempdir().unwrap();
    let key_file = dir.path().join("sa.json");
    std::fs::write(
        &key_file,
        json!({
            "type": "service_account",
            "project_id": "proj-from-key",
            "private_key_id": "kid-1",
            "private_key": private_pem,
            "client_email": "sa@proj.iam.gserviceaccount.com",
            "token_uri": token_uri,
        })
        .to_string(),
    )
    .unwrap();
    Fixture {
        _dir: dir,
        key_file,
        public_pem,
        token_uri,
    }
}

fn token_ok(token: &str, expires_in: u64) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(
        json!({"access_token": token, "expires_in": expires_in, "token_type": "Bearer"}),
    )
}

#[tokio::test]
async fn exchanges_signed_jwt_and_caches_token() {
    let server = MockServer::start().await;
    let fx = fixture(&server).await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .and(body_string_contains(
            "grant_type=urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Ajwt-bearer",
        ))
        .respond_with(token_ok("ya29.test", 3600))
        .expect(1)
        .mount(&server)
        .await;

    let auth = ServiceAccountAuth::new(Some(fx.key_file.clone()), "", reqwest::Client::new());
    assert_eq!(auth.get_token().await.unwrap(), "ya29.test");
    // 有效期充足时直接用缓存，不再请求
    assert_eq!(auth.get_token().await.unwrap(), "ya29.test");
    assert_eq!(auth.project_id(), "proj-from-key");
    assert_eq!(auth.last_error(), None);

    // 验证 JWT：RS256、kid、各项声明
    let requests: Vec<Request> = server.received_requests().await.unwrap();
    let form: Vec<(String, String)> = form_urlencoded::parse(&requests[0].body)
        .into_owned()
        .collect();
    let assertion = &form.iter().find(|(k, _)| k == "assertion").unwrap().1;
    let header = jsonwebtoken::decode_header(assertion).unwrap();
    assert_eq!(header.alg, Algorithm::RS256);
    assert_eq!(header.kid.as_deref(), Some("kid-1"));
    let mut validation = Validation::new(Algorithm::RS256);
    validation.set_audience(&[&fx.token_uri]);
    let claims = jsonwebtoken::decode::<Value>(
        assertion,
        &DecodingKey::from_rsa_pem(fx.public_pem.as_bytes()).unwrap(),
        &validation,
    )
    .unwrap()
    .claims;
    assert_eq!(claims["iss"], "sa@proj.iam.gserviceaccount.com");
    assert_eq!(claims["scope"], SCOPE);
    assert_eq!(
        claims["exp"].as_u64().unwrap() - claims["iat"].as_u64().unwrap(),
        3600
    );
}

#[tokio::test]
async fn refreshes_when_close_to_expiry() {
    let server = MockServer::start().await;
    let fx = fixture(&server).await;
    // 有效期不足 5 分钟：每次都要刷新
    Mock::given(path("/token"))
        .respond_with(token_ok("short", 60))
        .expect(2)
        .mount(&server)
        .await;
    let auth = ServiceAccountAuth::new(Some(fx.key_file.clone()), "", reqwest::Client::new());
    auth.get_token().await.unwrap();
    auth.get_token().await.unwrap();
}

#[tokio::test]
async fn concurrent_requests_refresh_once() {
    let server = MockServer::start().await;
    let fx = fixture(&server).await;
    Mock::given(path("/token"))
        .respond_with(token_ok("t", 3600).set_delay(std::time::Duration::from_millis(200)))
        .expect(1)
        .mount(&server)
        .await;
    let auth = std::sync::Arc::new(ServiceAccountAuth::new(
        Some(fx.key_file.clone()),
        "",
        reqwest::Client::new(),
    ));
    let tasks: Vec<_> = (0..8)
        .map(|_| {
            let auth = auth.clone();
            tokio::spawn(async move { auth.get_token().await.unwrap() })
        })
        .collect();
    for t in tasks {
        assert_eq!(t.await.unwrap(), "t");
    }
}

#[tokio::test]
async fn explicit_project_wins() {
    let server = MockServer::start().await;
    let fx = fixture(&server).await;
    Mock::given(path("/token"))
        .respond_with(token_ok("t", 3600))
        .mount(&server)
        .await;
    let auth = ServiceAccountAuth::new(Some(fx.key_file.clone()), "mine", reqwest::Client::new());
    auth.get_token().await.unwrap();
    assert_eq!(auth.project_id(), "mine");
}

#[tokio::test]
async fn token_endpoint_error_is_reported() {
    let server = MockServer::start().await;
    let fx = fixture(&server).await;
    Mock::given(path("/token"))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({"error": "invalid_grant"})))
        .mount(&server)
        .await;
    let auth = ServiceAccountAuth::new(Some(fx.key_file.clone()), "", reqwest::Client::new());
    let err = auth.get_token().await.unwrap_err();
    assert!(err.0.contains("invalid_grant"), "{err}");
    assert!(auth.last_error().is_some());
}

#[tokio::test]
async fn not_configured_and_missing_file() {
    let auth = ServiceAccountAuth::new(None, "", reqwest::Client::new());
    assert!(auth.get_token().await.unwrap_err().0.contains("未配置"));

    let auth = ServiceAccountAuth::new(
        Some(PathBuf::from("Z:/definitely/missing.json")),
        "",
        reqwest::Client::new(),
    );
    assert!(auth.get_token().await.unwrap_err().0.contains("无法读取"));
}
