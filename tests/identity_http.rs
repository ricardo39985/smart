//! Real HTTP, PostgreSQL and signed synthetic OIDC tokens; no live provider accounts.
use axum::{
    Form, Json, Router,
    routing::{get, post},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use smartcindy::{AppleIdentity, Application, Config, GoogleIdentity};
use std::collections::HashMap;
use testcontainers::{
    GenericImage, ImageExt,
    core::{IntoContainerPort, WaitFor},
    runners::AsyncRunner,
};

struct Task(tokio::task::JoinHandle<()>);
impl Drop for Task {
    fn drop(&mut self) {
        self.0.abort();
    }
}
struct World {
    base: String,
    database_url: String,
    client: reqwest::Client,
    _api: Task,
    _provider: Task,
    _db: testcontainers::ContainerAsync<GenericImage>,
}
impl World {
    async fn new() -> Self {
        let provider = Router::new().route("/{provider}/token", post(token)).route(
            "/{provider}/keys",
            get(|| async {
                Json(
                    serde_json::from_str::<Value>(include_str!("fixtures/oidc-test-jwks.json"))
                        .unwrap(),
                )
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/", listener.local_addr().unwrap());
        let provider = Task(tokio::spawn(async move {
            axum::serve(listener, provider).await.unwrap()
        }));
        let db = GenericImage::new(
            "pgvector/pgvector",
            "pg18@sha256:ad249f9fb9572979643a47694e59b9a8cdcdf6abc6accebf49bb80ad5f20ff9b",
        )
        .with_exposed_port(5432.tcp())
        .with_wait_for(WaitFor::message_on_stderr(
            "listening on IPv4 address \"0.0.0.0\", port 5432",
        ))
        .with_env_var("POSTGRES_PASSWORD", "synthetic-password")
        .start()
        .await
        .unwrap();
        let url = format!(
            "postgres://postgres:synthetic-password@127.0.0.1:{}/postgres",
            db.get_host_port_ipv4(5432.tcp()).await.unwrap()
        );
        Application::migrate(&url).await.unwrap();
        let mut cfg = Config::new(url.clone());
        cfg.identity.google = Some(GoogleIdentity {
            client_id: "synthetic-google".into(),
            client_secret: "synthetic-secret".into(),
        });
        cfg.identity.apple = Some(AppleIdentity {
            client_id: "synthetic-apple".into(),
            team_id: "SYNTHETIC".into(),
            key_id: "TEST".into(),
            private_key: include_str!("fixtures/oidc-public-test-ec.txt").into(),
        });
        cfg.identity.origin = Some("https://dev.smartcindy.app".into());
        cfg.identity.app_scheme = Some("smartcindy-dev".into());
        cfg.identity.local_simulator = Some(endpoint);
        let app = Application::connect(cfg).await.unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let api = Task(tokio::spawn(async move {
            axum::serve(listener, app.router()).await.unwrap()
        }));
        Self {
            base,
            database_url: url,
            client: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(std::time::Duration::from_secs(10))
                .build()
                .unwrap(),
            _api: api,
            _provider: provider,
            _db: db,
        }
    }
    async fn signup(&self, email: &str) -> Value {
        let r=self.client.post(format!("{}/v1/accounts",self.base)).json(&json!({"email":email,"display_name":"Test Person","password":"correct-password-123"})).send().await.unwrap();
        assert_eq!(r.status(), 201);
        r.json().await.unwrap()
    }
    async fn start(&self, provider: &str, session: Option<&str>) -> (String, String, Value) {
        let verifier = "native-proof-for-synthetic-identity-tests-1234567890123456789";
        let mut body =
            json!({"challenge":URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))});
        let mut req = self
            .client
            .post(format!("{}/v1/auth/{provider}/start", self.base));
        if let Some(s) = session {
            body["link"] = json!(true);
            body["confirm"] = json!(true);
            body["password"] = json!("correct-password-123");
            req = req.bearer_auth(s);
        }
        let r = req.json(&body).send().await.unwrap();
        assert_eq!(r.status(), 200);
        let v: Value = r.json().await.unwrap();
        let u = reqwest::Url::parse(v["authorization_url"].as_str().unwrap()).unwrap();
        let pairs: HashMap<String, String> = u.query_pairs().into_owned().collect();
        let now = chrono::Utc::now().timestamp();
        let claims = json!({"iss":if provider=="google"{"https://accounts.google.com"}else{"https://appleid.apple.com"},"aud":format!("synthetic-{provider}"),"exp":now+120,"iat":now,"sub":"provider-person","email":"provider@example.com","email_verified":true,"nonce":pairs["nonce"]});
        (pairs["state"].clone(), verifier.into(), claims)
    }
    async fn complete(&self, provider: &str, state: &str, claims: Value) -> String {
        let code = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).unwrap());
        let url = format!("{}/v1/auth/{provider}/callback", self.base);
        let r = if provider == "apple" {
            self.client
                .post(url)
                .form(&[("state", state), ("code", &code)])
                .send()
                .await
                .unwrap()
        } else {
            self.client
                .get(url)
                .query(&[("state", state), ("code", &code)])
                .send()
                .await
                .unwrap()
        };
        assert_eq!(r.status(), 303);
        assert_eq!(r.headers()["cache-control"], "no-store");
        let url = reqwest::Url::parse(r.headers()["location"].to_str().unwrap()).unwrap();
        assert_eq!(url.scheme(), "smartcindy-dev");
        assert_eq!(url.host_str(), Some("auth"));
        assert_eq!(url.path(), "/callback");
        assert_eq!(url.query_pairs().count(), 1);
        url.query_pairs()
            .find(|(k, _)| k == "code")
            .unwrap()
            .1
            .into_owned()
    }
    async fn exchange(&self, code: &str, verifier: &str) -> reqwest::Response {
        self.client
            .post(format!("{}/v1/auth/exchange", self.base))
            .json(&json!({"code":code,"verifier":verifier}))
            .send()
            .await
            .unwrap()
    }
    async fn account(&self, s: &str) -> Value {
        let r = self
            .client
            .get(format!("{}/v1/account", self.base))
            .bearer_auth(s)
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 200);
        r.json().await.unwrap()
    }
}
async fn token(Form(form): Form<HashMap<String, String>>) -> Json<Value> {
    let mut claims: Value =
        serde_json::from_slice(&URL_SAFE_NO_PAD.decode(&form["code"]).unwrap()).unwrap();
    let algorithm = claims
        .as_object_mut()
        .unwrap()
        .remove("test_algorithm")
        .and_then(|v| v.as_str().map(str::to_owned));
    let alg = if algorithm.as_deref() == Some("HS256") {
        Algorithm::HS256
    } else {
        Algorithm::RS256
    };
    let mut header = Header::new(alg);
    header.kid = Some("public-test-key".into());
    let key = if alg == Algorithm::HS256 {
        EncodingKey::from_secret(b"deliberately-public-synthetic-test-secret")
    } else {
        EncodingKey::from_rsa_pem(include_bytes!("fixtures/oidc-public-test-rsa.txt")).unwrap()
    };
    Json(
        json!({"id_token":jsonwebtoken::encode(&header,&claims,&key).unwrap(),"token_type":"Bearer"}),
    )
}

#[tokio::test]
async fn google_login_collision_and_explicit_link_keep_one_account() {
    let w = World::new().await;
    let local = w.signup("local@example.com").await;
    let local_token = local["token"].as_str().unwrap();
    let (state, verifier, mut claims) = w.start("google", None).await;
    claims["email"] = json!("local@example.com");
    let code = w.complete("google", &state, claims).await;
    let r = w.exchange(&code, &verifier).await;
    assert_eq!(r.status(), 409);
    assert_eq!(
        r.json::<Value>().await.unwrap()["error"]["code"],
        "account_link_required"
    );
    assert_eq!(w.account(local_token).await["methods"], json!(["password"]));
    let (state, verifier, mut claims) = w.start("google", Some(local_token)).await;
    claims["email"] = json!("local@example.com");
    let code = w.complete("google", &state, claims).await;
    let r = w.exchange(&code, &verifier).await;
    assert_eq!(r.status(), 200);
    let linked: Value = r.json().await.unwrap();
    assert_eq!(linked["linked"], true);
    assert_eq!(linked["id"], local["user_id"]);
    assert!(linked.get("token").is_none());
    assert_eq!(linked["methods"], json!(["password", "google"]));
    let (state, verifier, mut claims) = w.start("google", None).await;
    claims.as_object_mut().unwrap().remove("email");
    let code = w.complete("google", &state, claims).await;
    let r = w.exchange(&code, &verifier).await;
    assert_eq!(r.status(), 200);
    let signed: Value = r.json().await.unwrap();
    assert_eq!(signed["user_id"], local["user_id"]);
    let r = w
        .client
        .post(format!("{}/v1/sessions", w.base))
        .json(&json!({"email":"local@example.com","password":"correct-password-123"}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    assert_eq!(
        r.json::<Value>().await.unwrap()["user_id"],
        local["user_id"]
    );
    assert_eq!(w.exchange(&code, &verifier).await.status(), 400);
}

#[tokio::test]
async fn apple_provider_account_is_passwordless_and_subject_owned() {
    let w = World::new().await;
    let (state, verifier, claims) = w.start("apple", None).await;
    let code = w.complete("apple", &state, claims).await;
    let r = w.exchange(&code, &verifier).await;
    assert_eq!(r.status(), 200);
    let signed: Value = r.json().await.unwrap();
    assert_eq!(
        w.account(signed["token"].as_str().unwrap()).await["methods"],
        json!(["apple"])
    );
    let r = w
        .client
        .post(format!("{}/v1/sessions", w.base))
        .json(&json!({"email":"provider@example.com","password":"correct-password-123"}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 401);
    let local = w.signup("another@example.com").await;
    let (state, verifier, claims) = w
        .start("apple", Some(local["token"].as_str().unwrap()))
        .await;
    let code = w.complete("apple", &state, claims).await;
    let r = w.exchange(&code, &verifier).await;
    assert_eq!(r.status(), 409);
    assert_eq!(
        r.json::<Value>().await.unwrap()["error"]["code"],
        "identity_already_linked"
    );
    assert_eq!(
        w.account(local["token"].as_str().unwrap()).await["methods"],
        json!(["password"])
    );
}

#[tokio::test]
async fn revoked_session_cannot_finish_a_pending_identity_link() {
    let w = World::new().await;
    let local = w.signup("local@example.com").await;
    let session = local["token"].as_str().unwrap();
    let (state, verifier, claims) = w.start("google", Some(session)).await;
    let code = w.complete("google", &state, claims).await;
    assert_eq!(
        w.client
            .delete(format!("{}/v1/sessions/current", w.base))
            .bearer_auth(session)
            .send()
            .await
            .unwrap()
            .status(),
        204
    );
    assert_eq!(w.exchange(&code, &verifier).await.status(), 400);
    let r = w
        .client
        .post(format!("{}/v1/sessions", w.base))
        .json(&json!({"email":"local@example.com","password":"correct-password-123"}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let signed: Value = r.json().await.unwrap();
    assert_eq!(
        w.account(signed["token"].as_str().unwrap()).await["methods"],
        json!(["password"])
    );
}

#[tokio::test]
async fn invalid_signed_claims_and_algorithm_never_authenticate() {
    let w = World::new().await;
    for (field, value) in [
        ("iss", json!("https://attacker.invalid")),
        ("aud", json!("different-client")),
        ("exp", json!(1)),
        ("nonce", json!("wrong")),
        ("test_algorithm", json!("HS256")),
    ] {
        let (state, verifier, mut claims) = w.start("google", None).await;
        claims[field] = value;
        let code = w.complete("google", &state, claims).await;
        let r = w.exchange(&code, &verifier).await;
        assert_eq!(r.status(), 400);
        assert_eq!(
            r.json::<Value>().await.unwrap()["error"]["code"],
            "provider_unavailable"
        );
    }
}

#[tokio::test]
async fn verified_google_refresh_updates_provider_name_until_explicit_edit() {
    let w = World::new().await;
    let (state, verifier, mut claims) = w.start("google", None).await;
    claims["name"] = json!("  First Name  ");
    let code = w.complete("google", &state, claims).await;
    let signed: Value = w.exchange(&code, &verifier).await.json().await.unwrap();
    let session = signed["token"].as_str().unwrap();
    assert_eq!(w.account(session).await["display_name"], "First Name");
    assert_eq!(w.account(session).await["display_name_source"], "google");

    let (state, verifier, mut claims) = w.start("google", None).await;
    claims["name"] = json!("Second Name");
    claims["email"] = json!("second@example.com");
    let code = w.complete("google", &state, claims).await;
    assert_eq!(w.exchange(&code, &verifier).await.status(), 200);
    let profile = w.account(session).await;
    assert_eq!(profile["display_name"], "Second Name");
    assert_eq!(profile["email"], "provider@example.com");
    assert_eq!(profile["methods"], json!(["google"]));
}

#[tokio::test]
async fn missing_or_invalid_claims_do_not_erase_verified_profile() {
    let w = World::new().await;
    let (state, verifier, mut claims) = w.start("google", None).await;
    claims["name"] = json!("Maya Bennett");
    claims["picture"] = json!("https://lh3.googleusercontent.com/avatar");
    let code = w.complete("google", &state, claims).await;
    let signed: Value = w.exchange(&code, &verifier).await.json().await.unwrap();
    let session = signed["token"].as_str().unwrap();
    let (state, verifier, mut claims) = w.start("google", None).await;
    claims.as_object_mut().unwrap().remove("email");
    claims["name"] = json!("\u{0001}");
    claims["picture"] = json!("https://googleusercontent.com.evil.example/avatar");
    let code = w.complete("google", &state, claims).await;
    assert_eq!(w.exchange(&code, &verifier).await.status(), 200);
    let profile = w.account(session).await;
    assert_eq!(profile["display_name"], "Maya Bennett");
    assert_eq!(profile["email"], "provider@example.com");
    assert_eq!(
        profile["avatar_url"],
        "https://lh3.googleusercontent.com/avatar"
    );
}

#[tokio::test]
async fn explicit_link_preserves_local_name_and_password_email() {
    let w = World::new().await;
    let local = w.signup("local@example.com").await;
    let session = local["token"].as_str().unwrap();
    let (state, verifier, mut claims) = w.start("google", Some(session)).await;
    claims["name"] = json!("Provider Name");
    claims["email"] = json!("other-provider@example.com");
    claims["picture"] = json!("https://lh3.googleusercontent.com/new");
    let code = w.complete("google", &state, claims).await;
    let linked: Value = w.exchange(&code, &verifier).await.json().await.unwrap();
    assert_eq!(linked["linked"], true);
    assert_eq!(linked["display_name"], "Test Person");
    assert_eq!(linked["email"], "local@example.com");
    assert_eq!(
        linked["avatar_url"],
        "https://lh3.googleusercontent.com/new"
    );
    assert_eq!(linked["has_custom_avatar"], false);
    assert_eq!(
        w.account(session).await["methods"],
        json!(["password", "google"])
    );
}

#[tokio::test]
async fn invalid_and_oversized_photos_cannot_replace_private_photo() {
    let w = World::new().await;
    let local = w.signup("local@example.com").await;
    let session = local["token"].as_str().unwrap();
    let mut photo = std::io::Cursor::new(Vec::new());
    image::DynamicImage::new_rgb8(24, 24)
        .write_to(&mut photo, image::ImageFormat::Png)
        .unwrap();
    let original = photo.into_inner();
    let url = format!("{}/v1/account/avatar", w.base);
    let uploaded = w
        .client
        .put(&url)
        .bearer_auth(session)
        .header("content-type", "image/png")
        .body(original)
        .send()
        .await
        .unwrap();
    assert_eq!(uploaded.status(), 200);
    let saved = w
        .client
        .get(&url)
        .bearer_auth(session)
        .send()
        .await
        .unwrap()
        .bytes()
        .await
        .unwrap();
    assert_eq!(
        w.client
            .put(&url)
            .bearer_auth(session)
            .header("content-type", "image/png")
            .body(b"not an image".to_vec())
            .send()
            .await
            .unwrap()
            .status(),
        400
    );
    assert_eq!(
        w.client
            .put(&url)
            .bearer_auth(session)
            .header("content-type", "image/jpeg")
            .body(saved.clone())
            .send()
            .await
            .unwrap()
            .status(),
        400
    );
    assert_eq!(
        w.client
            .put(&url)
            .bearer_auth(session)
            .header("content-type", "image/png")
            .body(vec![0u8; 2 * 1024 * 1024 + 1])
            .send()
            .await
            .unwrap()
            .status(),
        413
    );
    assert_eq!(
        w.client
            .get(&url)
            .bearer_auth(session)
            .send()
            .await
            .unwrap()
            .bytes()
            .await
            .unwrap(),
        saved
    );
    assert_eq!(
        w.client
            .put(&url)
            .header("content-type", "image/png")
            .body(saved.clone())
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    assert_eq!(w.client.delete(&url).send().await.unwrap().status(), 401);
}

#[tokio::test]
async fn confirmed_relink_refreshes_google_picture_without_changing_local_name() {
    let w = World::new().await;
    let local = w.signup("local@example.com").await;
    let session = local["token"].as_str().unwrap();
    let (state, verifier, claims) = w.start("google", Some(session)).await;
    let code = w.complete("google", &state, claims).await;
    assert_eq!(w.exchange(&code, &verifier).await.status(), 200);
    let (state, verifier, mut claims) = w.start("google", Some(session)).await;
    claims["picture"] = json!("https://lh3.googleusercontent.com/refreshed");
    claims["name"] = json!("New Google Name");
    let code = w.complete("google", &state, claims).await;
    let refreshed: Value = w.exchange(&code, &verifier).await.json().await.unwrap();
    assert_eq!(refreshed["id"], local["user_id"]);
    assert_eq!(refreshed["linked"], true);
    assert_eq!(refreshed["display_name"], "Test Person");
    assert_eq!(
        refreshed["avatar_url"],
        "https://lh3.googleusercontent.com/refreshed"
    );
}

#[tokio::test]
async fn refreshed_provider_email_reserves_address_without_merging_new_signup() {
    let w = World::new().await;
    let (state, verifier, claims) = w.start("google", None).await;
    let code = w.complete("google", &state, claims).await;
    let original: Value = w.exchange(&code, &verifier).await.json().await.unwrap();
    let (state, verifier, mut claims) = w.start("google", None).await;
    claims["email"] = json!("refreshed@example.com");
    let code = w.complete("google", &state, claims).await;
    let next: Value = w.exchange(&code, &verifier).await.json().await.unwrap();
    assert_eq!(next["user_id"], original["user_id"]);
    assert_eq!(
        w.account(original["token"].as_str().unwrap()).await["email"],
        "provider@example.com"
    );
    let signup = w.client.post(format!("{}/v1/accounts", w.base))
        .json(&json!({"email":"refreshed@example.com","display_name":"Another person","password":"correct-password-123"}))
        .send().await.unwrap();
    assert_eq!(signup.status(), 409);
    let (state, verifier, mut claims) = w.start("apple", None).await;
    claims["email"] = json!("refreshed@example.com");
    let code = w.complete("apple", &state, claims).await;
    assert_eq!(w.exchange(&code, &verifier).await.status(), 409);
}

#[tokio::test]
async fn private_avatar_replacement_and_deletion_only_affect_owner() {
    let w = World::new().await;
    let first = w.signup("first@example.com").await;
    let second = w.signup("second@example.com").await;
    let owner = first["token"].as_str().unwrap();
    let stranger = second["token"].as_str().unwrap();
    let url = format!("{}/v1/account/avatar", w.base);
    let mut first_photo = std::io::Cursor::new(Vec::new());
    image::DynamicImage::new_rgb8(24, 24)
        .write_to(&mut first_photo, image::ImageFormat::Png)
        .unwrap();
    assert_eq!(
        w.client
            .put(&url)
            .bearer_auth(owner)
            .header("content-type", "image/png")
            .body(first_photo.into_inner())
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    assert_eq!(
        w.client
            .get(&url)
            .bearer_auth(stranger)
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
    let mut second_photo = std::io::Cursor::new(Vec::new());
    image::DynamicImage::new_rgb8(36, 12)
        .write_to(&mut second_photo, image::ImageFormat::Png)
        .unwrap();
    let updated = w
        .client
        .put(&url)
        .bearer_auth(owner)
        .header("content-type", "image/png")
        .body(second_photo.into_inner())
        .send()
        .await
        .unwrap();
    assert_eq!(updated.status(), 200);
    assert_eq!(
        updated.json::<Value>().await.unwrap()["has_custom_avatar"],
        true
    );
    let received = w.client.get(&url).bearer_auth(owner).send().await.unwrap();
    assert_eq!(received.headers()["content-type"], "image/png");
    assert_eq!(received.headers()["x-content-type-options"], "nosniff");
    let decoded = image::load_from_memory(&received.bytes().await.unwrap()).unwrap();
    assert_eq!((decoded.width(), decoded.height()), (36, 12));
    // Profile bytes have their own lifetime; deleting a chat cannot erase them.
    let chat = w
        .client
        .post(format!("{}/v1/chats", w.base))
        .bearer_auth(owner)
        .json(&json!({"title":"Disposable chat"}))
        .send()
        .await
        .unwrap();
    assert_eq!(chat.status(), 201);
    let chat: Value = chat.json().await.unwrap();
    let deleted = w
        .client
        .delete(format!(
            "{}/v1/chats/{}",
            w.base,
            chat["id"].as_str().unwrap()
        ))
        .bearer_auth(owner)
        .query(&[("version", chat["version"].as_i64().unwrap())])
        .send()
        .await
        .unwrap();
    assert_eq!(deleted.status(), 204);
    let avatar = w.client.get(&url).bearer_auth(owner).send().await.unwrap();
    assert_eq!(avatar.status(), 200);
    let decoded = image::load_from_memory(&avatar.bytes().await.unwrap()).unwrap();
    assert_eq!((decoded.width(), decoded.height()), (36, 12));
    assert_eq!(
        w.client
            .delete(&url)
            .bearer_auth(stranger)
            .send()
            .await
            .unwrap()
            .status(),
        204
    );
    assert_eq!(w.account(owner).await["has_custom_avatar"], true);
    assert_eq!(
        w.client
            .delete(&url)
            .bearer_auth(owner)
            .send()
            .await
            .unwrap()
            .status(),
        204
    );
    assert_eq!(
        w.client
            .get(&url)
            .bearer_auth(owner)
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
    assert_eq!(w.account(owner).await["has_custom_avatar"], false);
}

#[tokio::test]
async fn google_profile_refresh_preserves_local_edits_and_custom_photo() {
    let w = World::new().await;
    let (state, verifier, mut claims) = w.start("google", None).await;
    claims["name"] = json!("Maya Bennett");
    claims["picture"] = json!("https://lh3.googleusercontent.com/maya-original");
    let code = w.complete("google", &state, claims).await;
    let response = w.exchange(&code, &verifier).await;
    assert_eq!(response.status(), 200);
    let signed: Value = response.json().await.unwrap();
    let session = signed["token"].as_str().unwrap();
    let account = w.account(session).await;
    assert_eq!(account["display_name"], "Maya Bennett");
    assert_eq!(account["email"], "provider@example.com");
    assert_eq!(
        account["avatar_url"],
        "https://lh3.googleusercontent.com/maya-original"
    );
    assert_eq!(account["has_custom_avatar"], false);

    let update = w
        .client
        .patch(format!("{}/v1/account", w.base))
        .bearer_auth(session)
        .json(&json!({"display_name":"Maya at home"}))
        .send()
        .await
        .unwrap();
    assert_eq!(update.status(), 200);
    let mut photo = std::io::Cursor::new(Vec::new());
    image::DynamicImage::new_rgb8(24, 24)
        .write_to(&mut photo, image::ImageFormat::Png)
        .unwrap();
    let upload = w
        .client
        .put(format!("{}/v1/account/avatar", w.base))
        .bearer_auth(session)
        .header("content-type", "image/png")
        .body(photo.into_inner())
        .send()
        .await
        .unwrap();
    assert_eq!(upload.status(), 200);
    assert_eq!(w.account(session).await["has_custom_avatar"], true);
    let saved = w
        .client
        .get(format!("{}/v1/account/avatar", w.base))
        .bearer_auth(session)
        .send()
        .await
        .unwrap();
    assert_eq!(saved.status(), 200);
    assert_eq!(saved.headers()["cache-control"], "no-store");
    let saved_bytes = saved.bytes().await.unwrap();
    let decoded = image::load_from_memory(&saved_bytes).unwrap();
    assert_eq!((decoded.width(), decoded.height()), (24, 24));

    // A later provider sign-in refreshes verified attributes, never a Cindy override.
    let (state, verifier, mut claims) = w.start("google", None).await;
    claims["name"] = json!("Maya Updated");
    claims["email"] = json!("updated@example.com");
    claims["picture"] = json!("https://lh3.googleusercontent.com/maya-updated");
    let code = w.complete("google", &state, claims).await;
    let response = w.exchange(&code, &verifier).await;
    assert_eq!(response.status(), 200);
    let signed_again: Value = response.json().await.unwrap();
    assert_eq!(signed_again["user_id"], signed["user_id"]);
    let account = w.account(session).await;
    assert_eq!(account["display_name"], "Maya at home");
    assert_eq!(account["email"], "provider@example.com");
    assert_eq!(
        account["avatar_url"],
        "https://lh3.googleusercontent.com/maya-updated"
    );
    assert_eq!(account["has_custom_avatar"], true);
    assert_eq!(
        w.client
            .get(format!("{}/v1/account/avatar", w.base))
            .bearer_auth(session)
            .send()
            .await
            .unwrap()
            .bytes()
            .await
            .unwrap(),
        saved_bytes
    );

    let other = w.signup("someone-else@example.com").await;
    let other_token = other["token"].as_str().unwrap();
    assert_eq!(
        w.client
            .get(format!("{}/v1/account/avatar", w.base))
            .bearer_auth(other_token)
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
    assert_eq!(
        w.client
            .get(format!("{}/v1/account/avatar", w.base))
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    assert_eq!(
        w.client
            .delete(format!("{}/v1/account/avatar", w.base))
            .bearer_auth(session)
            .send()
            .await
            .unwrap()
            .status(),
        204
    );
    assert_eq!(w.account(session).await["has_custom_avatar"], false);
    assert_eq!(
        w.client
            .get(format!("{}/v1/account/avatar", w.base))
            .bearer_auth(session)
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
}

#[tokio::test]
async fn email_and_password_replacement_stay_disabled_for_local_and_provider_accounts() {
    let w = World::new().await;
    let local = w.signup("locked@example.com").await;
    let local_token = local["token"].as_str().unwrap();
    let (state, verifier, claims) = w.start("google", None).await;
    let code = w.complete("google", &state, claims).await;
    let signed: Value = w.exchange(&code, &verifier).await.json().await.unwrap();
    let provider_token = signed["token"].as_str().unwrap();
    for session in [local_token, provider_token] {
        let before = w.account(session).await;
        assert_eq!(
            before["capabilities"],
            json!({"email_change":false,"password_change":false})
        );
        for (path, body) in [
            ("email-change", json!({"email":"replacement@example.com"})),
            (
                "password-change",
                json!({"current_password":"correct-password-123","password":"changed-password-456"}),
            ),
        ] {
            let route = format!("{}/v1/account/{path}", w.base);
            let refused = w
                .client
                .post(&route)
                .bearer_auth(session)
                .json(&body)
                .send()
                .await
                .unwrap();
            assert_eq!(refused.status(), 503);
            assert_eq!(
                refused.json::<Value>().await.unwrap()["error"]["code"],
                "email_delivery_unavailable"
            );
            assert_eq!(
                w.client
                    .post(route)
                    .json(&body)
                    .send()
                    .await
                    .unwrap()
                    .status(),
                401
            );
        }
        assert_eq!(w.account(session).await, before);
        assert_eq!(
            w.client
                .patch(format!("{}/v1/account", w.base))
                .bearer_auth(session)
                .json(&json!({"display_name":"Allowed name","email":"replacement@example.com"}))
                .send()
                .await
                .unwrap()
                .status(),
            400
        );
    }
    // Disabled replacement neither changes the existing password nor installs one
    // on an account authenticated only by its third-party subject.
    assert_eq!(
        w.client
            .post(format!("{}/v1/sessions", w.base))
            .json(&json!({"email":"locked@example.com","password":"correct-password-123"}))
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    assert_eq!(
        w.client
            .post(format!("{}/v1/sessions", w.base))
            .json(&json!({"email":"locked@example.com","password":"changed-password-456"}))
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    assert_eq!(
        w.client
            .post(format!("{}/v1/sessions", w.base))
            .json(&json!({"email":"provider@example.com","password":"changed-password-456"}))
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
}

#[tokio::test]
async fn phone_photo_orientation_survives_private_metadata_removal() {
    let w = World::new().await;
    let signed = w.signup("photo-orientation@example.com").await;
    let session = signed["token"].as_str().unwrap();
    let mut encoded = std::io::Cursor::new(Vec::new());
    image::DynamicImage::new_rgb8(3, 2)
        .write_to(&mut encoded, image::ImageFormat::Jpeg)
        .unwrap();
    let jpeg = encoded.into_inner();
    // Synthetic APP1/EXIF: orientation 6 means the displayed photo is 2 by 3.
    let mut photo = jpeg[..2].to_vec();
    photo.extend_from_slice(&[
        0xff, 0xe1, 0x00, 0x22, b'E', b'x', b'i', b'f', 0, 0, b'I', b'I', 42, 0, 8, 0, 0, 0, 1, 0,
        0x12, 0x01, 3, 0, 1, 0, 0, 0, 6, 0, 0, 0, 0, 0, 0, 0,
    ]);
    photo.extend_from_slice(&jpeg[2..]);
    let url = format!("{}/v1/account/avatar", w.base);
    let uploaded = w
        .client
        .put(&url)
        .bearer_auth(session)
        .header("content-type", "image/jpeg")
        .body(photo)
        .send()
        .await
        .unwrap();
    assert_eq!(uploaded.status(), 200);
    let received = w
        .client
        .get(&url)
        .bearer_auth(session)
        .send()
        .await
        .unwrap();
    assert_eq!(received.status(), 200);
    assert_eq!(received.headers()["content-type"], "image/png");
    let bytes = received.bytes().await.unwrap();
    let decoded = image::load_from_memory(&bytes).unwrap();
    assert_eq!((decoded.width(), decoded.height()), (2, 3));
    assert!(!bytes.windows(4).any(|chunk| chunk == b"eXIf"));
}

#[tokio::test]
async fn another_google_identity_cannot_replace_link_and_consumes_confirmation() {
    let w = World::new().await;
    let local = w.signup("linked@example.com").await;
    let session = local["token"].as_str().unwrap();
    let (state, verifier, claims) = w.start("google", Some(session)).await;
    let code = w.complete("google", &state, claims).await;
    assert_eq!(w.exchange(&code, &verifier).await.status(), 200);
    let before = w.account(session).await;

    let (state, verifier, mut claims) = w.start("google", Some(session)).await;
    claims["sub"] = json!("different-google-person");
    claims["email"] = json!("different-google@example.com");
    claims["name"] = json!("Different person");
    claims["picture"] = json!("https://lh3.googleusercontent.com/different");
    let code = w.complete("google", &state, claims).await;
    let refused = w.exchange(&code, &verifier).await;
    assert_eq!(refused.status(), 409);
    assert_eq!(
        refused.json::<Value>().await.unwrap()["error"]["code"],
        "identity_already_linked"
    );
    assert_eq!(w.account(session).await, before);
    assert_eq!(w.exchange(&code, &verifier).await.status(), 400);

    // The original Google identity still signs into the same account.
    let (state, verifier, claims) = w.start("google", None).await;
    let code = w.complete("google", &state, claims).await;
    let signed: Value = w.exchange(&code, &verifier).await.json().await.unwrap();
    assert_eq!(signed["user_id"], local["user_id"]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn private_photo_dimensions_remain_bounded_without_enlarging_or_distorting() {
    use proptest::{
        prelude::*,
        test_runner::{Config as PropertyConfig, RngSeed, TestRunner},
    };
    let w = World::new().await;
    let signed = w.signup("photo-dimensions@example.com").await;
    let session = signed["token"].as_str().unwrap();
    let url = format!("{}/v1/account/avatar", w.base);
    let mut runner = TestRunner::new(PropertyConfig {
        cases: 16,
        rng_seed: RngSeed::Fixed(73641990),
        ..PropertyConfig::default()
    });
    let handle = tokio::runtime::Handle::current();
    tokio::task::block_in_place(|| {
        runner.run(&(1u32..=1024, 1u32..=768), |(width, height)| {
            handle.block_on(async {
                let mut encoded = std::io::Cursor::new(Vec::new());
                image::DynamicImage::new_rgb8(width, height)
                    .write_to(&mut encoded, image::ImageFormat::Png)
                    .unwrap();
                let uploaded = w
                    .client
                    .put(&url)
                    .bearer_auth(session)
                    .header("content-type", "image/png")
                    .body(encoded.into_inner())
                    .send()
                    .await
                    .unwrap();
                prop_assert_eq!(uploaded.status().as_u16(), 200);
                let received = w
                    .client
                    .get(&url)
                    .bearer_auth(session)
                    .send()
                    .await
                    .unwrap();
                prop_assert_eq!(received.status().as_u16(), 200);
                let bytes = received.bytes().await.unwrap();
                prop_assert_eq!(
                    image::guess_format(&bytes).unwrap(),
                    image::ImageFormat::Png
                );
                let image = image::load_from_memory(&bytes).unwrap();
                let (out_width, out_height) = (image.width(), image.height());
                prop_assert!((1..=512).contains(&out_width));
                prop_assert!((1..=512).contains(&out_height));
                prop_assert!(out_width <= width && out_height <= height);
                prop_assert_eq!(out_width.max(out_height), width.max(height).min(512));
                // Pixel rounding may alter the aspect ratio by less than one pixel.
                let aspect_error = (i64::from(out_width) * i64::from(height)
                    - i64::from(out_height) * i64::from(width))
                .abs();
                prop_assert!(aspect_error <= i64::from(width.max(height)));
                Ok(())
            })
        })
    })
    .unwrap();
}

// The database is used only to hold/release the address reservation barrier and
// observe the cross-table uniqueness invariant. Both competing actions use HTTP.
async fn wait_for_two_address_reservations(pool: &sqlx::PgPool) {
    tokio::time::timeout(std::time::Duration::from_secs(8), async {
        loop {
            let waiting: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM pg_stat_activity WHERE datname=current_database() AND wait_event_type='Lock' AND wait_event='advisory'",
            ).fetch_one(pool).await.unwrap();
            if waiting >= 2 {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }).await.expect("both HTTP operations must reach the reservation barrier");
}

#[tokio::test]
async fn signup_racing_verified_email_refresh_keeps_one_address_owner() {
    let w = World::new().await;
    let (state, verifier, claims) = w.start("google", None).await;
    let code = w.complete("google", &state, claims).await;
    let original: Value = w.exchange(&code, &verifier).await.json().await.unwrap();
    let (state, verifier, mut claims) = w.start("google", None).await;
    claims["email"] = json!("race@example.com");
    let code = w.complete("google", &state, claims).await;
    let pool = sqlx::PgPool::connect(&w.database_url).await.unwrap();
    let mut barrier = pool.begin().await.unwrap();
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('race@example.com', 0))")
        .execute(&mut *barrier)
        .await
        .unwrap();
    let refresh = w
        .client
        .post(format!("{}/v1/auth/exchange", w.base))
        .json(&json!({"code":code,"verifier":verifier}));
    let signup = w.client.post(format!("{}/v1/accounts", w.base))
        .json(&json!({"email":"  RACE@example.com  ","display_name":"Racing person","password":"correct-password-123"}));
    let refresh = tokio::spawn(async move { refresh.send().await.unwrap() });
    let signup = tokio::spawn(async move { signup.send().await.unwrap() });
    wait_for_two_address_reservations(&pool).await;
    barrier.commit().await.unwrap();
    let refreshed = refresh.await.unwrap();
    assert_eq!(refreshed.status(), 200);
    let refreshed: Value = refreshed.json().await.unwrap();
    assert_eq!(refreshed["user_id"], original["user_id"]);
    assert!([201, 409].contains(&signup.await.unwrap().status().as_u16()));
    // Whichever serialized first owns this address; a refresh must not publish
    // its verified alias if the simultaneous local signup reserved it first.
    let owners: i64 = sqlx::query_scalar(
        "SELECT count(DISTINCT owner) FROM (SELECT id AS owner FROM users WHERE email='race@example.com' UNION ALL SELECT user_id AS owner FROM account_identities WHERE verified_email='race@example.com') addresses",
    ).fetch_one(&pool).await.unwrap();
    assert_eq!(owners, 1);
    assert_eq!(
        w.account(original["token"].as_str().unwrap()).await["email"],
        "provider@example.com"
    );
    let (state, verifier, mut claims) = w.start("apple", None).await;
    claims["email"] = json!("race@example.com");
    let code = w.complete("apple", &state, claims).await;
    assert_eq!(w.exchange(&code, &verifier).await.status(), 409);
}
