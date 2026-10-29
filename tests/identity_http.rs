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
