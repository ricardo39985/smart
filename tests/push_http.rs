//! Real PostgreSQL and HTTP native-push contracts; provider requests use local simulators.
use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    routing::post,
};
use proptest::strategy::{Strategy, ValueTree};
use serde_json::{Value, json};
use smartcindy::{ApnsConfig, Application, Config, FcmConfig, ProviderConfig, PushConfig};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use testcontainers::{
    ContainerAsync, GenericImage, ImageExt,
    core::{IntoContainerPort, WaitFor},
    runners::AsyncRunner,
};
use uuid::Uuid;

type Received = Arc<Mutex<Vec<(String, Value, HeaderMap)>>>;
async fn fcm(
    State(received): State<Received>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    assert_eq!(headers["authorization"], "Bearer synthetic-access-token");
    let mut content = body["message"].clone();
    content.as_object_mut().unwrap().remove("token");
    assert!(serde_json::to_vec(&content).unwrap().len() <= 4096);
    let invalid = body["message"]["token"] == "invalid-registration";
    received
        .lock()
        .unwrap()
        .push(("android".into(), body, headers));
    if invalid {
        (
            StatusCode::NOT_FOUND,
            Json(json!({"error":{"details":[{"errorCode":"UNREGISTERED"}]}})),
        )
    } else {
        (
            StatusCode::OK,
            Json(json!({"name":"projects/synthetic-project/messages/message"})),
        )
    }
}
async fn apns(
    State(received): State<Received>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> StatusCode {
    assert!(
        headers["authorization"]
            .to_str()
            .unwrap()
            .starts_with("Bearer ey")
    );
    assert_eq!(headers["apns-push-type"], "alert");
    assert_eq!(headers["apns-topic"], "app.smartcindy.test");
    assert!(serde_json::to_vec(&body).unwrap().len() <= 4096);
    received.lock().unwrap().push(("ios".into(), body, headers));
    StatusCode::OK
}
struct Harness {
    _db: ContainerAsync<GenericImage>,
    pool: sqlx::PgPool,
    app: Application,
    client: reqwest::Client,
    base: String,
    received: Received,
    server: tokio::task::JoinHandle<()>,
    provider: tokio::task::JoinHandle<()>,
}
impl Drop for Harness {
    fn drop(&mut self) {
        self.server.abort();
        self.provider.abort();
    }
}
impl Harness {
    async fn new(push_enabled: bool) -> Self {
        let db = GenericImage::new(
            "pgvector/pgvector",
            "pg18@sha256:ad249f9fb9572979643a47694e59b9a8cdcdf6abc6accebf49bb80ad5f20ff9b",
        )
        .with_exposed_port(5432.tcp())
        .with_wait_for(WaitFor::message_on_stderr(
            "listening on IPv4 address \"0.0.0.0\", port 5432",
        ))
        .with_env_var("POSTGRES_PASSWORD", "synthetic-push-password")
        .start()
        .await
        .expect("Docker required; never skip");
        let url = format!(
            "postgres://postgres:synthetic-push-password@127.0.0.1:{}/postgres",
            db.get_host_port_ipv4(5432.tcp()).await.unwrap()
        );
        Application::migrate(&url).await.unwrap();
        let received: Received = Arc::new(Mutex::new(Vec::new()));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let provider_router=Router::new()
            .route("/oauth/token",post(|body:String|async move{
                assert!(body.contains("grant_type=urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Ajwt-bearer&assertion=ey"));
                Json(json!({"access_token":"synthetic-access-token","expires_in":3600}))
            }))
            .route("/v1/projects/synthetic-project/messages:send",post(fcm))
            .route("/3/device/{token}",post(apns))
            .route("/chat/completions",post(||async{ ([("content-type","text/event-stream")],"data: {\"choices\":[{\"delta\":{\"content\":\"Private reply about confidential travel plans.\"},\"finish_reason\":null}]}\n\ndata: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n") }))
            .with_state(received.clone());
        let provider = tokio::spawn(async move {
            axum::serve(listener, provider_router).await.unwrap();
        });
        let mut config = Config::new(url.clone());
        config.identity.origin = Some("https://dev.smartcindy.app".into());
        config.identity.app_scheme = Some("smartcindy-dev".into());
        config.providers = ProviderConfig {
            deepseek_key: Some("synthetic-key".into()),
            bfl_key: None,
            local_simulator: Some(origin.clone()),
        };
        if push_enabled {
            config.push = PushConfig {
                fcm: Some(FcmConfig {
                    project_id: "synthetic-project".into(),
                    client_email: "synthetic@synthetic-project.iam.gserviceaccount.com".into(),
                    private_key: include_str!("fixtures/oidc-public-test-rsa.txt").into(),
                }),
                apns: Some(ApnsConfig {
                    team_id: "TEAM123456".into(),
                    key_id: "KEY1234567".into(),
                    bundle_id: "app.smartcindy.test".into(),
                    private_key: include_str!("fixtures/oidc-public-test-ec.txt").into(),
                    sandbox: true,
                }),
                local_simulator: Some(origin),
            };
        }
        let pool = sqlx::PgPool::connect(&url).await.unwrap();
        let app = Application::new(pool.clone(), config).await.unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let cloned = app.clone();
        let server = tokio::spawn(async move {
            axum::serve(listener, cloned.router()).await.unwrap();
        });
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap();
        Self {
            _db: db,
            pool,
            app,
            client,
            base,
            received,
            server,
            provider,
        }
    }
    async fn signup(&self, email: &str) -> String {
        let response=self.client.post(format!("{}/v1/accounts",self.base)).json(&json!({"email":email,"password":"synthetic-password-123","display_name":"Push User"})).send().await.unwrap();
        assert_eq!(response.status(), 201);
        response.json::<Value>().await.unwrap()["token"]
            .as_str()
            .unwrap()
            .into()
    }
    async fn register(&self, token: &str, id: Uuid, body: Value, status: u16) -> Value {
        let response = self
            .client
            .put(format!("{}/v1/devices/{id}", self.base))
            .bearer_auth(token)
            .json(&body)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), status);
        response.json().await.unwrap()
    }
    async fn turn(&self, token: &str) -> String {
        let response = self
            .client
            .post(format!("{}/v1/chats", self.base))
            .bearer_auth(token)
            .json(&json!({"title":"Private title must not be pushed"}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 201);
        let chat = response.json::<Value>().await.unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let response=self.client.post(format!("{}/v1/chats/{chat}/turns",self.base)).bearer_auth(token).json(&json!({"request_id":Uuid::new_v4(),"parent_id":null,"parts":[{"type":"text","text":"Private prompt must not be pushed"}],"output":"text","replaces_message_id":null})).send().await.unwrap();
        assert_eq!(response.status(), 202);
        chat
    }
    async fn wait_pushes(&self, count: usize) {
        tokio::time::timeout(Duration::from_secs(8), async {
            loop {
                if self.received.lock().unwrap().len() >= count {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
    }
    async fn wait_disabled(&self, device: Uuid) {
        // Invalid-token retirement is a durable delivery contract with no public device GET.
        tokio::time::timeout(Duration::from_secs(8), async {
            loop {
                let enabled: bool = sqlx::query_scalar("SELECT enabled FROM devices WHERE id=$1")
                    .bind(device)
                    .fetch_one(&self.pool)
                    .await
                    .unwrap();
                if !enabled {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
    }
    async fn wait_reply(&self, token: &str, chat: &str) {
        tokio::time::timeout(Duration::from_secs(8), async {
            loop {
                let value: Value = self
                    .client
                    .get(format!("{}/v1/chats/{chat}/messages", self.base))
                    .bearer_auth(token)
                    .send()
                    .await
                    .unwrap()
                    .json()
                    .await
                    .unwrap();
                if value["items"]
                    .as_array()
                    .is_some_and(|messages| messages.len() == 2)
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
    }
    async fn wait_lock_waiters(&self, blocker: i32, minimum: i64) {
        // PostgreSQL lock metadata makes the public DELETE/completion race deterministic;
        // this barrier does not inspect application query text or private call ordering.
        tokio::time::timeout(Duration::from_secs(8), async {
            loop {
                let count: i64 = sqlx::query_scalar("WITH RECURSIVE waiting(pid) AS (SELECT pid FROM pg_stat_activity WHERE $1=ANY(pg_blocking_pids(pid)) UNION SELECT a.pid FROM pg_stat_activity a JOIN waiting w ON w.pid=ANY(pg_blocking_pids(a.pid))) SELECT count(*) FROM waiting")
                    .bind(blocker).fetch_one(&self.pool).await.unwrap();
                if count >= minimum { break; }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        }).await.unwrap();
    }
}

#[tokio::test]
async fn device_removal_during_completion_preserves_saved_answer() {
    let h = Harness::new(true).await;
    let owner = h.signup("push-removal-race@example.com").await;
    let device = Uuid::new_v4();
    h.register(
        &owner,
        device,
        json!({"platform":"android","token":"remove-during-completion"}),
        200,
    )
    .await;

    // Hold only the recipient row so DELETE reaches its lock before completion.
    // The actual removal and paid generation both go through their public APIs.
    let mut barrier = h.pool.begin().await.unwrap();
    let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *barrier)
        .await
        .unwrap();
    sqlx::query("SELECT id FROM devices WHERE id=$1 FOR UPDATE")
        .bind(device)
        .fetch_one(&mut *barrier)
        .await
        .unwrap();
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .unwrap();
    let url = format!("{}/v1/devices/{device}", h.base);
    let deleting_owner = owner.clone();
    let deletion = tokio::spawn(async move {
        client
            .delete(url)
            .bearer_auth(deleting_owner)
            .send()
            .await
            .unwrap()
    });
    h.wait_lock_waiters(blocker, 1).await;
    let chat = h.turn(&owner).await;
    let worker = h.app.run_worker();
    h.wait_lock_waiters(blocker, 2).await;
    barrier.commit().await.unwrap();

    assert_eq!(deletion.await.unwrap().status(), 204);
    h.wait_reply(&owner, &chat).await;
    assert!(h.received.lock().unwrap().is_empty());
    worker.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_locked_expired_push_does_not_block_other_completions_or_replace_a_fresh_lease() {
    let h = Harness::new(true).await;
    let owner = h.signup("push-lease-race@example.com").await;
    h.register(
        &owner,
        Uuid::new_v4(),
        json!({"platform":"android","token":"lease-race"}),
        200,
    )
    .await;
    let first = h.turn(&owner).await;
    let worker = h.app.run_worker();
    h.wait_pushes(1).await;
    worker.shutdown().await.unwrap();

    // Recovery fixture: an existing public completion's delivery lease has expired.
    // Hold that row as another process would while renewing/reclaiming its lease.
    let id: Uuid = sqlx::query_scalar("SELECT id FROM push_outbox WHERE chat_id=$1")
        .bind(Uuid::parse_str(&first).unwrap())
        .fetch_one(&h.pool)
        .await
        .unwrap();
    sqlx::query("UPDATE push_outbox SET status='sending',attempts=1,lease=$2,lease_until=now()-interval '1 second' WHERE id=$1")
        .bind(id).bind(Uuid::new_v4()).execute(&h.pool).await.unwrap();
    let mut barrier = h.pool.begin().await.unwrap();
    sqlx::query("SELECT id FROM push_outbox WHERE id=$1 FOR UPDATE")
        .bind(id)
        .fetch_one(&mut *barrier)
        .await
        .unwrap();
    let worker = h.app.run_worker();
    let second = h.turn(&owner).await;
    h.wait_reply(&owner, &second).await;
    h.wait_pushes(2).await;

    let fresh_lease = Uuid::new_v4();
    sqlx::query("UPDATE push_outbox SET lease=$2,lease_until=now()+interval '1 hour' WHERE id=$1")
        .bind(id)
        .bind(fresh_lease)
        .execute(&mut *barrier)
        .await
        .unwrap();
    barrier.commit().await.unwrap();
    h.turn(&owner).await;
    h.wait_pushes(3).await;
    let lease: (String, Option<Uuid>) =
        sqlx::query_as("SELECT status,lease FROM push_outbox WHERE id=$1")
            .bind(id)
            .fetch_one(&h.pool)
            .await
            .unwrap();
    assert_eq!(lease, ("sending".into(), Some(fresh_lease)));
    worker.shutdown().await.unwrap();
}

#[tokio::test]
async fn devices_are_private_session_scoped_and_preview_optin() {
    let h = Harness::new(false).await;
    let owner = h.signup("push-owner@example.com").await;
    let other = h.signup("push-other@example.com").await;
    let id = Uuid::new_v4();
    let saved = h
        .register(
            &owner,
            id,
            json!({"platform":"android","token":"private-device-token"}),
            200,
        )
        .await;
    assert_eq!(saved["previews"], false);
    assert_eq!(saved["enabled"], true);
    assert!(saved.get("token").is_none());
    // Retrying an identical PUT is a no-op, including its persisted update timestamp.
    let repeated = h
        .register(
            &owner,
            id,
            json!({"platform":"android","token":"private-device-token"}),
            200,
        )
        .await;
    assert_eq!(repeated, saved);
    h.register(
        &other,
        id,
        json!({"platform":"android","token":"private-device-token"}),
        409,
    )
    .await;
    let response = h
        .client
        .delete(format!("{}/v1/devices/{id}", h.base))
        .bearer_auth(&other)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 404);
    let changed=h.register(&owner,id,json!({"platform":"android","token":"private-device-token","enabled":false,"previews":true}),200).await;
    assert_eq!(changed["enabled"], false);
    assert_eq!(changed["previews"], true);
    for body in [
        json!({"platform":"web","token":"x"}),
        json!({"platform":"ios","token":"not-hex"}),
        json!({"platform":"android","token":""}),
        json!({"platform":"android","token":"x".repeat(4097)}),
        json!({"platform":"android","token":"has spaces"}),
    ] {
        h.register(&owner, Uuid::new_v4(), body, 400).await;
    }
    // A token's hexadecimal spelling must never bypass exclusive recipient ownership.
    let mut runner = proptest::test_runner::TestRunner::new(proptest::test_runner::Config {
        cases: 8,
        rng_seed: proptest::test_runner::RngSeed::Fixed(73591245),
        ..Default::default()
    });
    for _ in 0..8 {
        let token = "[a-f0-9]{16,128}".new_tree(&mut runner).unwrap().current();
        let device = Uuid::new_v4();
        h.register(&owner, device, json!({"platform":"ios","token":token}), 200)
            .await;
        h.register(
            &other,
            Uuid::new_v4(),
            json!({"platform":"ios","token":token.to_ascii_uppercase()}),
            409,
        )
        .await;
        assert_eq!(
            h.client
                .delete(format!("{}/v1/devices/{device}", h.base))
                .bearer_auth(&owner)
                .send()
                .await
                .unwrap()
                .status(),
            204
        );
    }
    let response = h
        .client
        .delete(format!("{}/v1/sessions/current", h.base))
        .bearer_auth(&owner)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 204);
    h.register(
        &owner,
        id,
        json!({"platform":"android","token":"private-device-token"}),
        401,
    )
    .await;
    // Logout removed the recipient registration; another signed-in owner can now register it.
    h.register(
        &other,
        id,
        json!({"platform":"android","token":"private-device-token"}),
        200,
    )
    .await;
    let response = h
        .client
        .delete(format!("{}/v1/devices/{id}", h.base))
        .bearer_auth(&other)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 204);
    for n in 0..32 {
        h.register(
            &other,
            Uuid::new_v4(),
            json!({"platform":"android","token":format!("device-{n}")}),
            200,
        )
        .await;
    }
    h.register(
        &other,
        Uuid::new_v4(),
        json!({"platform":"android","token":"over-quota"}),
        429,
    )
    .await;
    // Expired sessions cannot be revived by a client; expire the fixture clock state,
    // then verify a fresh account can recover the provider token through the public API.
    use sha2::{Digest, Sha256};
    sqlx::query("UPDATE sessions SET expires_at=now()-interval '1 second' WHERE token_hash=$1")
        .bind(Sha256::digest(other.as_bytes()).to_vec())
        .execute(&h.pool)
        .await
        .unwrap();
    let fresh = h.signup("push-recovery@example.com").await;
    h.register(
        &fresh,
        Uuid::new_v4(),
        json!({"platform":"android","token":"device-0"}),
        200,
    )
    .await;
}

#[tokio::test]
async fn completion_push_uses_rich_metadata_and_opt_in_previews() {
    let h = Harness::new(true).await;
    let owner = h.signup("rich-push@example.com").await;
    let android = Uuid::new_v4();
    let ios = Uuid::new_v4();
    h.register(
        &owner,
        android,
        json!({"platform":"android","token":"android-token"}),
        200,
    )
    .await;
    h.register(
        &owner,
        ios,
        json!({"platform":"ios","token":"abcd1234"}),
        200,
    )
    .await;
    let chat = h.turn(&owner).await;
    let worker = h.app.run_worker();
    h.wait_pushes(2).await;
    {
        let received = h.received.lock().unwrap();
        assert_eq!(received.len(), 2);
        let (_, fcm, _) = received
            .iter()
            .find(|(platform, _, _)| platform == "android")
            .unwrap();
        let (_, apns, headers) = received
            .iter()
            .find(|(platform, _, _)| platform == "ios")
            .unwrap();
        assert_eq!(
            fcm["message"]["notification"]["body"],
            "Open your conversation."
        );
        assert_eq!(
            fcm["message"]["android"]["notification"]["channel_id"],
            "conversation_updates"
        );
        assert_eq!(fcm["message"]["data"]["chat_id"], chat);
        assert!(
            fcm["message"]["data"]["deep_link"]
                .as_str()
                .unwrap()
                .starts_with(&format!("smartcindy-dev://chat/{chat}?generation="))
        );
        let actions: Value =
            serde_json::from_str(fcm["message"]["data"]["actions"].as_str().unwrap()).unwrap();
        assert!(
            actions
                .as_array()
                .unwrap()
                .contains(&json!({"id":"REPLY","title":"Reply","input":"text"}))
        );
        assert!(
            fcm["message"]["data"]["actions"]
                .as_str()
                .unwrap()
                .contains("COPY_REPLY")
        );
        assert_eq!(apns["aps"]["category"], "CHAT_REPLY");
        assert!(
            apns["deep_link"]
                .as_str()
                .unwrap()
                .starts_with(&format!("smartcindy-dev://chat/{chat}?generation="))
        );
        assert_eq!(apns["aps"]["interruption-level"], "active");
        assert_eq!(headers["apns-collapse-id"].to_str().unwrap(), chat);
        for (_, body, _) in received.iter() {
            let body = body.to_string();
            assert!(!body.contains("Private reply"));
            assert!(!body.contains("Private title"));
            assert!(!body.contains("Private prompt"));
            assert!(!body.contains("attachment_id"));
        }
    }
    h.register(
        &owner,
        android,
        json!({"platform":"android","token":"android-token","previews":true}),
        200,
    )
    .await;
    h.register(
        &owner,
        ios,
        json!({"platform":"ios","token":"abcd1234","enabled":false}),
        200,
    )
    .await;
    h.turn(&owner).await;
    h.wait_pushes(3).await;
    {
        let received = h.received.lock().unwrap();
        assert_eq!(received[2].0, "android");
        assert_eq!(
            received[2].1["message"]["notification"]["body"],
            "Private reply about confidential travel plans."
        );
    }
    // An invalid provider registration is disabled and does not repeatedly receive later replies.
    h.register(
        &owner,
        android,
        json!({"platform":"android","token":"invalid-registration"}),
        200,
    )
    .await;
    h.turn(&owner).await;
    h.wait_pushes(4).await;
    h.wait_disabled(android).await;
    let ignored_chat = h.turn(&owner).await;
    h.wait_reply(&owner, &ignored_chat).await;
    assert_eq!(h.received.lock().unwrap().len(), 4);
    let queued: i64 = sqlx::query_scalar("SELECT count(*) FROM push_outbox WHERE chat_id=$1")
        .bind(Uuid::parse_str(&ignored_chat).unwrap())
        .fetch_one(&h.pool)
        .await
        .unwrap();
    assert_eq!(queued, 0);
    worker.shutdown().await.unwrap();
}
