//! Public HTTP journeys against production migrations/PostgreSQL 18.
//! Only paid HTTP boundaries are simulated; no private application imports.
use axum::{
    Json, Router,
    extract::State,
    response::{
        IntoResponse,
        sse::{Event, Sse},
    },
    routing::{get, post},
};
use futures_util::StreamExt;
use serde_json::{Value, json};
use smartcindy::{Application, Config, DeepSeekReasoning, ProviderConfig};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use testcontainers::{
    ContainerAsync, GenericImage, ImageExt,
    core::{IntoContainerPort, WaitFor},
    runners::AsyncRunner,
};
use uuid::Uuid;

const DIGEST: &str = "sha256:ad249f9fb9572979643a47694e59b9a8cdcdf6abc6accebf49bb80ad5f20ff9b";
#[derive(Clone, Default)]
struct Simulator {
    requests: Arc<Mutex<Vec<Value>>>,
    image_requests: Arc<Mutex<Vec<Value>>>,
    image_calls: Arc<AtomicUsize>,
    origin: Arc<Mutex<String>>,
}
async fn text_provider(
    State(sim): State<Simulator>,
    Json(body): Json<Value>,
) -> axum::response::Response {
    sim.requests.lock().unwrap().push(body.clone());
    let content = body.to_string();
    if content.contains("[fragmented]") {
        let bytes=b": heartbeat\r\ndata: {\"choices\":\r\ndata: [{\"delta\":{\"content\":\"Hello \"},\"finish_reason\":null}]}\r\n\r\ndata: {\"choices\":[{\"delta\":{\"content\":\"world\"},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":6,\"completion_tokens\":3,\"total_tokens\":9,\"vendor_detail\":\"not-public\"}}\r\n\r\ndata: [DONE]\r\n\r\n";
        let stream = async_stream::stream! {for chunk in bytes.chunks(3){yield Ok::<_,std::convert::Infallible>(axum::body::Bytes::copy_from_slice(chunk));tokio::task::yield_now().await;}};
        return (
            [("content-type", "text/event-stream")],
            axum::body::Body::from_stream(stream),
        )
            .into_response();
    }
    let blocked = content.contains("[blocked]");
    let truncated = content.contains("[truncated]");
    let malformed = content.contains("[malformed]");
    let stream = async_stream::stream! {
        // Includes reasoning which must not escape into the durable transcript/events.
        yield Ok::<Event,std::convert::Infallible>(Event::default().data(json!({"choices":[{"delta":{"content":"Hello ","reasoning_content":"PRIVATE_REASONING"},"finish_reason":null}]}).to_string()));
        if blocked {std::future::pending::<()>().await;}
        if malformed {yield Ok(Event::default().data("{broken"));return;}
        yield Ok(Event::default().data(json!({"choices":[{"delta":{"content":"world"},"finish_reason":null}]}).to_string()));
        if truncated {return;}
        yield Ok(Event::default().data(json!({"choices":[{"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":6,"completion_tokens":3,"total_tokens":9,"vendor_detail":"not-public"}}).to_string()));
        yield Ok(Event::default().data("[DONE]"));
    };
    Sse::new(stream).into_response()
}
async fn image_provider(State(sim): State<Simulator>, Json(body): Json<Value>) -> Json<Value> {
    assert_eq!(body["width"], 1024);
    assert_eq!(body["height"], 1024);
    sim.image_requests.lock().unwrap().push(body);
    sim.image_calls.fetch_add(1, Ordering::SeqCst);
    Json(
        json!({"id":"synthetic-receipt","polling_url":format!("{}/poll",sim.origin.lock().unwrap())}),
    )
}
async fn poll(State(sim): State<Simulator>) -> Json<Value> {
    Json(
        json!({"status":"Ready","result":{"sample":format!("{}/sample",sim.origin.lock().unwrap())}}),
    )
}
fn png() -> Vec<u8> {
    let mut out = std::io::Cursor::new(Vec::new());
    image::DynamicImage::new_rgb8(2, 3)
        .write_to(&mut out, image::ImageFormat::Png)
        .unwrap();
    out.into_inner()
}
async fn sample(headers: axum::http::HeaderMap) -> impl IntoResponse {
    assert!(!headers.contains_key("x-key"));
    assert!(!headers.contains_key("authorization"));
    ([("content-type", "image/png")], png())
}
struct Harness {
    _db: ContainerAsync<GenericImage>,
    database_url: String,
    app: Application,
    base: String,
    client: reqwest::Client,
    sim: Simulator,
    server: tokio::task::JoinHandle<()>,
    simulator: tokio::task::JoinHandle<()>,
}
impl Drop for Harness {
    fn drop(&mut self) {
        self.server.abort();
        self.simulator.abort();
    }
}
impl Harness {
    async fn new() -> Self {
        let db = GenericImage::new("pgvector/pgvector", &format!("pg18@{DIGEST}"))
            .with_exposed_port(5432.tcp())
            .with_wait_for(WaitFor::message_on_stderr(
                "listening on IPv4 address \"0.0.0.0\", port 5432",
            ))
            .with_env_var("POSTGRES_PASSWORD", "synthetic-only-password")
            .start()
            .await
            .expect("Docker/PostgreSQL required; do not skip");
        let port = db.get_host_port_ipv4(5432.tcp()).await.unwrap();
        let url = format!("postgres://postgres:synthetic-only-password@127.0.0.1:{port}/postgres");
        Application::migrate(&url).await.unwrap();
        let sim = Simulator::default();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        *sim.origin.lock().unwrap() = origin.clone();
        let routes = Router::new()
            .route("/chat/completions", post(text_provider))
            .route("/v1/flux-2-klein-4b", post(image_provider))
            .route("/poll", get(poll))
            .route("/sample", get(sample))
            .with_state(sim.clone());
        let simulator = tokio::spawn(async move {
            axum::serve(listener, routes).await.unwrap();
        });
        let mut config = Config::new(url.clone());
        config.providers = ProviderConfig {
            deepseek_key: Some("synthetic-text".into()),
            bfl_key: Some("synthetic-image".into()),
            local_simulator: Some(origin),
        };
        let app = Application::connect(config).await.unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let routes = app.router();
        let server = tokio::spawn(async move {
            axum::serve(listener, routes).await.unwrap();
        });
        Self {
            _db: db,
            database_url: url,
            app,
            base,
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(15))
                .build()
                .unwrap(),
            sim,
            server,
            simulator,
        }
    }
    async fn account(&self, email: &str) -> String {
        self.app
            .provision_user(email, "synthetic-password-123".into())
            .await
            .unwrap();
        let response = self
            .client
            .post(format!("{}/v1/sessions", self.base))
            .json(&json!({"email":email,"password":"synthetic-password-123"}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        response.json::<Value>().await.unwrap()["token"]
            .as_str()
            .unwrap()
            .into()
    }
    async fn create(&self, token: &str, title: &str) -> Value {
        let r = self
            .client
            .post(format!("{}/v1/chats", self.base))
            .bearer_auth(token)
            .json(&json!({"title":title}))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 201);
        r.json().await.unwrap()
    }
    async fn get(&self, token: &str, path: &str) -> Value {
        let r = self
            .client
            .get(format!("{}{path}", self.base))
            .bearer_auth(token)
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 200);
        assert_eq!(r.headers()["cache-control"], "no-store");
        r.json().await.unwrap()
    }
    async fn submit(&self, token: &str, chat: &str, payload: Value) -> Value {
        let r = self
            .client
            .post(format!("{}/v1/chats/{chat}/turns", self.base))
            .bearer_auth(token)
            .json(&payload)
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 202);
        r.json().await.unwrap()
    }
    async fn terminal(&self, token: &str, id: &str) -> Value {
        tokio::time::timeout(Duration::from_secs(12), async {
            loop {
                let value = self.get(token, &format!("/v1/generations/{id}")).await;
                if !matches!(value["status"].as_str(), Some("running" | "queued")) {
                    return value;
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        })
        .await
        .expect("generation deadline")
    }
    async fn calls(&self, n: usize) {
        tokio::time::timeout(Duration::from_secs(8), async {
            loop {
                if self.sim.requests.lock().unwrap().len() >= n {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
    }
    async fn replay(&self, token: &str, id: &str, after: i64) -> String {
        let r = self
            .client
            .get(format!("{}/v1/generations/{id}/events", self.base))
            .bearer_auth(token)
            .header("Last-Event-ID", after.to_string())
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 200);
        r.text().await.unwrap()
    }
}
fn turn(text: &str, parent: Option<&str>) -> Value {
    json!({"request_id":Uuid::new_v4(),"parent_id":parent,"parts":[{"type":"text","text":text}],"output":"text","replaces_message_id":null})
}
fn id(value: &Value, key: &str) -> String {
    value[key].as_str().unwrap().into()
}
fn event_ids(events: &str) -> Vec<i64> {
    events
        .lines()
        .filter_map(|line| line.strip_prefix("id: "))
        .map(|s| s.parse().unwrap())
        .collect()
}

async fn hold_streams(h: &Harness, token: &str, generation: &str) -> Vec<reqwest::Response> {
    let mut held = Vec::new();
    for _ in 0..32 {
        let response = h
            .client
            .get(format!("{}/v1/generations/{generation}/events", h.base))
            .bearer_auth(token)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        held.push(response);
    }
    held
}

#[tokio::test]
async fn server_controls_flash_reasoning_without_client_model_selection() {
    let h = Harness::new().await;
    let token = h.account("reasoning@example.com").await;
    let chat = h.create(&token, "Internal policy").await;
    let id = chat["id"].as_str().unwrap();
    let worker = h.app.run_worker();
    for (effort, label, thinking) in [
        (DeepSeekReasoning::None, "none", "disabled"),
        (DeepSeekReasoning::Low, "low", "enabled"),
        (DeepSeekReasoning::High, "high", "enabled"),
        (DeepSeekReasoning::Max, "max", "enabled"),
    ] {
        h.app.set_text_reasoning(effort);
        let accepted = h.submit(&token, id, json!({"request_id":Uuid::new_v4(),"output":"text","parts":[{"type":"text","text":"Hello"}]})).await;
        let generation = accepted["generation_id"].as_str().unwrap();
        let result = h.terminal(&token, generation).await;
        assert_eq!(result["status"], "completed");
        assert!(result.get("model").is_none());
        let request = h.sim.requests.lock().unwrap().last().unwrap().clone();
        assert_eq!(request["model"], "deepseek-flash");
        assert_eq!(request["reasoning_effort"], label);
        assert_eq!(request["thinking"]["type"], thinking);
        assert_eq!(request["max_tokens"], 8192);
        let events = h.replay(&token, generation, 0).await;
        assert!(!events.contains("PRIVATE_REASONING"));
    }
    for field in ["model", "reasoning_effort"] {
        let mut body = json!({"request_id":Uuid::new_v4(),"output":"text","parts":[{"type":"text","text":"Hello"}]});
        body[field] = json!("client-choice");
        let response = h
            .client
            .post(format!("{}/v1/chats/{id}/turns", h.base))
            .bearer_auth(&token)
            .json(&body)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 400);
    }
    assert_eq!(h.sim.requests.lock().unwrap().len(), 4);
    worker.shutdown().await.unwrap();
}

#[tokio::test]
async fn stream_subscribers_are_bounded_and_disconnect_releases_capacity() {
    let h = Harness::new().await;
    let a = h.account("streams@example.test").await;
    let c = id(&h.create(&a, "stream bounds").await, "id");
    let accepted = h.submit(&a, &c, turn("queued", None)).await;
    let generation = id(&accepted, "generation_id");
    let held = hold_streams(&h, &a, &generation).await;
    let overflow = h
        .client
        .get(format!("{}/v1/generations/{generation}/events", h.base))
        .bearer_auth(&a)
        .send()
        .await
        .unwrap();
    assert_eq!(
        overflow.status(),
        429,
        "live subscribers must have a finite admission bound"
    );
    drop(held);
    let reopened = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let r = h
                .client
                .get(format!("{}/v1/generations/{generation}/events", h.base))
                .bearer_auth(&a)
                .send()
                .await
                .unwrap();
            if r.status() == 200 {
                return r;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(reopened.status(), 200);
}

#[tokio::test]
async fn authenticated_durable_turn_replay_retry_and_scoped_ownership() {
    let h = Harness::new().await;
    let a = h.account("a@example.test").await;
    let b = h.account("b@example.test").await;
    let rejected = h
        .client
        .post(format!("{}/v1/sessions", h.base))
        .json(&json!({"email":"a@example.test","password":"incorrect-password"}))
        .send()
        .await
        .unwrap();
    assert_eq!(rejected.status(), 401);
    assert!(
        rejected
            .json::<Value>()
            .await
            .unwrap()
            .get("token")
            .is_none()
    );
    let unknown = h
        .client
        .post(format!("{}/v1/sessions", h.base))
        .json(&json!({"email":"unknown@example.test","password":"synthetic-password-123"}))
        .send()
        .await
        .unwrap();
    assert_eq!(unknown.status(), 401);
    assert_eq!(
        h.client
            .get(format!("{}/v1/chats", h.base))
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    assert_eq!(
        h.get(&a, "/v1/capabilities").await,
        json!({"text":true,"image_input":true,"image_output":true,"image_editing":true,"push":false,"push_android":false,"push_ios":false})
    );
    let invalid = h
        .client
        .post(format!("{}/v1/chats", h.base))
        .bearer_auth(&a)
        .json(&json!({"title":"invalid","unknown_field":true}))
        .send()
        .await
        .unwrap();
    assert_eq!(
        invalid.status(),
        400,
        "unknown request fields follow the documented invalid-request contract"
    );
    let chat = h.create(&a, "durable").await;
    let cid = id(&chat, "id");
    assert_eq!(h.get(&a, &format!("/v1/chats/{cid}")).await, chat);
    assert_eq!(
        h.client
            .get(format!("{}/v1/chats/{cid}", h.base))
            .bearer_auth(&b)
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
    let payload = turn("hi [fragmented]", None);
    // Concurrent identical requests are one durable acceptance, even before a worker starts.
    let url = format!("{}/v1/chats/{cid}/turns", h.base);
    let (one, two) = tokio::join!(
        h.client.post(&url).bearer_auth(&a).json(&payload).send(),
        h.client.post(&url).bearer_auth(&a).json(&payload).send()
    );
    let one = one.unwrap();
    let two = two.unwrap();
    assert_eq!(one.status(), 202);
    assert_eq!(two.status(), 202);
    let accepted: Value = one.json().await.unwrap();
    let duplicate: Value = two.json().await.unwrap();
    assert_eq!(accepted, duplicate);
    let gid = id(&accepted, "generation_id");
    let mut changed = payload.clone();
    changed["parts"][0]["text"] = json!("different");
    assert_eq!(
        h.client
            .post(&url)
            .bearer_auth(&a)
            .json(&changed)
            .send()
            .await
            .unwrap()
            .status(),
        409
    );
    assert_eq!(
        h.client
            .post(&url)
            .bearer_auth(&a)
            .json(&turn("conflict", None))
            .send()
            .await
            .unwrap()
            .status(),
        409
    );
    let worker = h.app.run_worker();
    let done = h.terminal(&a, &gid).await;
    assert_eq!(done["status"], "completed");
    assert_eq!(
        done["usage"],
        json!({"input_tokens":6,"output_tokens":3,"total_tokens":9})
    );
    let all = h.replay(&a, &gid, 0).await;
    let ids = event_ids(&all);
    assert_eq!(ids.first(), Some(&1));
    assert!(ids.windows(2).all(|p| p[1] == p[0] + 1));
    assert!(all.contains("completed"));
    assert!(!all.contains("PRIVATE_REASONING"));
    let replay = h.replay(&a, &gid, 1).await;
    assert_eq!(event_ids(&replay), ids[1..]);
    let history = h
        .get(&a, &format!("/v1/chats/{cid}/messages?limit=1"))
        .await;
    assert_eq!(history["items"][0]["parts"][0]["text"], "Hello world");
    let cursor = history["next_cursor"].as_str().unwrap();
    let older = h
        .get(
            &a,
            &format!("/v1/chats/{cid}/messages?limit=1&cursor={cursor}"),
        )
        .await;
    assert_eq!(older["items"][0]["parts"][0]["text"], "hi [fragmented]");
    assert_eq!(older["head_id"], history["head_id"]);
    // Recovery through a newly connected Application, not only the original in-memory instance.
    worker.shutdown().await.unwrap();
    let restarted = Application::connect(Config::new(h.database_url.clone()))
        .await
        .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let restart_base = format!("http://{}", listener.local_addr().unwrap());
    let routes = restarted.router();
    let server = tokio::spawn(async move {
        axum::serve(listener, routes).await.unwrap();
    });
    let retry = h
        .client
        .post(format!("{restart_base}/v1/chats/{cid}/turns"))
        .bearer_auth(&a)
        .json(&payload)
        .send()
        .await
        .unwrap();
    assert_eq!(retry.status(), 202);
    assert_eq!(retry.json::<Value>().await.unwrap()["generation_id"], gid);
    let recovered = h
        .client
        .get(format!("{restart_base}/v1/chats/{cid}/messages"))
        .bearer_auth(&a)
        .send()
        .await
        .unwrap();
    assert_eq!(recovered.status(), 200);
    assert_eq!(
        recovered.json::<Value>().await.unwrap()["items"][0]["parts"][0]["text"],
        "Hello world"
    );
    server.abort();
    assert_eq!(h.sim.requests.lock().unwrap().len(), 1);
    for path in [
        format!("/v1/chats/{cid}/messages"),
        format!("/v1/chats/{cid}/branches"),
        format!("/v1/generations/{gid}"),
    ] {
        assert_eq!(
            h.client
                .get(format!("{}{path}", h.base))
                .bearer_auth(&b)
                .send()
                .await
                .unwrap()
                .status(),
            404
        );
    }
    assert_eq!(
        h.client
            .patch(format!("{}/v1/chats/{cid}", h.base))
            .bearer_auth(&b)
            .json(&json!({"version":1,"title":"steal"}))
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
    assert_eq!(
        h.client
            .post(format!("{}/v1/generations/{gid}/cancel", h.base))
            .bearer_auth(&b)
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
    assert_eq!(
        h.client
            .delete(format!("{}/v1/sessions/current", h.base))
            .bearer_auth(&a)
            .send()
            .await
            .unwrap()
            .status(),
        204
    );
    assert_eq!(
        h.client
            .get(format!("{}/v1/chats", h.base))
            .bearer_auth(&a)
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn chat_head_generation_lookup_stays_on_the_selected_message() {
    let h = Harness::new().await;
    let owner = h.account("head-owner@example.test").await;
    let other = h.account("head-other@example.test").await;
    let chat = h.create(&owner, "head lookup").await;
    let chat_id = id(&chat, "id");
    let worker = h.app.run_worker();
    assert_eq!(
        h.get(&owner, &format!("/v1/chats/{chat_id}/generation"))
            .await["generation"],
        Value::Null
    );
    let accepted = h.submit(&owner, &chat_id, turn("hello", None)).await;
    let generation_id = id(&accepted, "generation_id");
    let user_message_id = id(&accepted, "user_message_id");
    let user_generation = h
        .get(
            &owner,
            &format!("/v1/chats/{chat_id}/generation?head_id={user_message_id}"),
        )
        .await;
    assert_eq!(
        user_generation["generation"]["generation_id"],
        generation_id
    );
    h.terminal(&owner, &generation_id).await;
    let current = h
        .get(&owner, &format!("/v1/chats/{chat_id}/generation"))
        .await;
    assert_eq!(current["generation"]["generation_id"], generation_id);
    assert_eq!(current["generation"]["status"], "completed");
    let messages = h
        .get(&owner, &format!("/v1/chats/{chat_id}/messages"))
        .await;
    let assistant_id = messages["items"][0]["id"].as_str().unwrap();
    let assistant_generation = h
        .get(
            &owner,
            &format!("/v1/chats/{chat_id}/generation?head_id={assistant_id}"),
        )
        .await;
    assert_eq!(
        assistant_generation["generation"]["generation_id"],
        generation_id
    );
    let regeneration = h
        .client
        .post(format!(
            "{}/v1/chats/{chat_id}/messages/{user_message_id}/regenerations",
            h.base
        ))
        .bearer_auth(&owner)
        .json(&json!({"request_id":Uuid::new_v4(),"output":"text"}))
        .send()
        .await
        .unwrap();
    assert_eq!(regeneration.status(), 202);
    let second_generation: Value = regeneration.json().await.unwrap();
    let second_generation_id = id(&second_generation, "generation_id");
    h.terminal(&owner, &second_generation_id).await;
    let latest_user_attempt = h
        .get(
            &owner,
            &format!("/v1/chats/{chat_id}/generation?head_id={user_message_id}"),
        )
        .await;
    assert_eq!(
        latest_user_attempt["generation"]["generation_id"],
        second_generation_id
    );
    let old_assistant_branch = h
        .get(
            &owner,
            &format!("/v1/chats/{chat_id}/generation?head_id={assistant_id}"),
        )
        .await;
    assert_eq!(
        old_assistant_branch["generation"]["generation_id"],
        generation_id
    );
    let foreign = h.create(&other, "foreign head").await;
    let foreign_head = id(&foreign, "id");
    let response = h
        .client
        .get(format!(
            "{}/v1/chats/{chat_id}/generation?head_id={foreign_head}",
            h.base
        ))
        .bearer_auth(&owner)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 404);
    assert_eq!(h.sim.requests.lock().unwrap().len(), 2);
    worker.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn chat_pagination_snapshot_and_public_cursor_properties() {
    use proptest::{
        prelude::*,
        test_runner::{Config as PropertyConfig, RngSeed, TestRunner},
    };
    let h = Harness::new().await;
    let a = h.account("a@example.test").await;
    let b = h.account("b@example.test").await;
    let oldest = h.create(&a, "oldest").await;
    let middle = h.create(&a, "middle").await;
    let newest = h.create(&a, "newest").await;
    let first = h.get(&a, "/v1/chats?limit=1").await;
    assert_eq!(first["items"][0]["id"], newest["id"]);
    let inserted = h.create(&a, "new insertion").await;
    assert_eq!(
        h.get(&a, "/v1/chats?limit=1").await["items"][0]["id"],
        inserted["id"]
    );
    let search = h.get(&a, "/v1/chats?q=%20EST%20&limit=1").await;
    assert_eq!(search["items"][0]["id"], newest["id"]);
    let search_cursor = search["next_cursor"].as_str().unwrap();
    let search_next = h
        .get(
            &a,
            &format!("/v1/chats?q=est&limit=1&cursor={search_cursor}"),
        )
        .await;
    assert_eq!(search_next["items"][0]["id"], oldest["id"]);
    let changed_filter = h
        .client
        .get(format!(
            "{}/v1/chats?q=literal&cursor={search_cursor}",
            h.base
        ))
        .bearer_auth(&a)
        .send()
        .await
        .unwrap();
    assert_eq!(changed_filter.status(), 400);
    let literal = h.create(&a, "100% literal").await;
    let literal_search = h.get(&a, "/v1/chats?q=%25&limit=10").await;
    assert_eq!(literal_search["items"].as_array().unwrap().len(), 1);
    assert_eq!(literal_search["items"][0]["id"], literal["id"]);
    let cursor = first["next_cursor"].as_str().unwrap();
    let second = h
        .get(&a, &format!("/v1/chats?limit=1&cursor={cursor}"))
        .await;
    assert_eq!(second["items"][0]["id"], middle["id"]);
    let third = h
        .get(
            &a,
            &format!(
                "/v1/chats?limit=1&cursor={}",
                second["next_cursor"].as_str().unwrap()
            ),
        )
        .await;
    assert_eq!(third["items"][0]["id"], oldest["id"]);
    assert!(third["next_cursor"].is_null());
    assert_eq!(
        h.client
            .get(format!("{}/v1/chats?archived=true&cursor={cursor}", h.base))
            .bearer_auth(&a)
            .send()
            .await
            .unwrap()
            .status(),
        400
    );
    assert_eq!(
        h.client
            .get(format!("{}/v1/chats?cursor={cursor}", h.base))
            .bearer_auth(&b)
            .send()
            .await
            .unwrap()
            .status(),
        400
    );
    // Broad limits/cursor validation through public HTTP. Fixed seed, 24 bounded cases.
    let mut runner = TestRunner::new(PropertyConfig {
        cases: 24,
        rng_seed: RngSeed::Fixed(73641981),
        ..PropertyConfig::default()
    });
    let handle = tokio::runtime::Handle::current();
    tokio::task::block_in_place(|| {
        runner.run(&(1i64..=100, "[!@#$%^&*]{1,30}"), |(limit, bad_cursor)| {
            handle.block_on(async {
                let page = h.get(&a, &format!("/v1/chats?limit={limit}")).await;
                prop_assert!(page["items"].as_array().unwrap().len() <= limit as usize);
                let r = h
                    .client
                    .get(format!("{}/v1/chats", h.base))
                    .bearer_auth(&a)
                    .query(&[("cursor", bad_cursor)])
                    .send()
                    .await
                    .unwrap();
                prop_assert_eq!(r.status().as_u16(), 400);
                Ok(())
            })
        })
    })
    .unwrap();
    assert_eq!(
        h.client
            .get(format!("{}/v1/chats?limit=0", h.base))
            .bearer_auth(&a)
            .send()
            .await
            .unwrap()
            .status(),
        400
    );
    assert_eq!(
        h.client
            .get(format!("{}/v1/chats?limit=101", h.base))
            .bearer_auth(&a)
            .send()
            .await
            .unwrap()
            .status(),
        400
    );
}

#[tokio::test]
async fn edited_and_regenerated_branches_preserve_old_history_and_context() {
    let h = Harness::new().await;
    let a = h.account("a@example.test").await;
    let c = id(&h.create(&a, "branches").await, "id");
    let w = h.app.run_worker();
    let first = h.submit(&a, &c, turn("original-question", None)).await;
    let g = id(&first, "generation_id");
    let original_user = id(&first, "user_message_id");
    let answer = id(&h.terminal(&a, &g).await, "result_message_id");
    let continuation = h
        .submit(&a, &c, turn("old-descendant", Some(&answer)))
        .await;
    let old_head = id(
        &h.terminal(&a, &id(&continuation, "generation_id")).await,
        "result_message_id",
    );
    let mut edit = turn("replacement-question", None);
    edit["replaces_message_id"] = json!(original_user);
    let edited = h.submit(&a, &c, edit).await;
    let new_user = id(&edited, "user_message_id");
    let new_head = id(
        &h.terminal(&a, &id(&edited, "generation_id")).await,
        "result_message_id",
    );
    {
        let requests = h.sim.requests.lock().unwrap();
        assert!(requests[2].to_string().contains("replacement-question"));
        assert!(!requests[2].to_string().contains("original-question"));
        assert!(!requests[2].to_string().contains("old-descendant"));
    }
    let old = h
        .get(&a, &format!("/v1/chats/{c}/messages?head_id={old_head}"))
        .await;
    assert_eq!(old["items"].as_array().unwrap().len(), 4);
    assert_eq!(old["items"][3]["parts"][0]["text"], "original-question");
    let r = h
        .client
        .post(format!(
            "{}/v1/chats/{c}/messages/{new_user}/regenerations",
            h.base
        ))
        .bearer_auth(&a)
        .json(&json!({"request_id":Uuid::new_v4(),"output":"text"}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 202);
    let regen: Value = r.json().await.unwrap();
    let alternative = id(
        &h.terminal(&a, &id(&regen, "generation_id")).await,
        "result_message_id",
    );
    assert_ne!(alternative, new_head);
    let branches = h.get(&a, &format!("/v1/chats/{c}/branches?limit=1")).await;
    assert_eq!(branches["items"][0]["id"], alternative);
    assert!(branches["next_cursor"].is_string());
    let old_page = h
        .get(
            &a,
            &format!("/v1/chats/{c}/messages?head_id={old_head}&limit=1"),
        )
        .await;
    assert_eq!(
        h.client
            .get(format!("{}/v1/chats/{c}/messages", h.base))
            .bearer_auth(&a)
            .query(&[
                ("head_id", new_head.as_str()),
                ("cursor", old_page["next_cursor"].as_str().unwrap())
            ])
            .send()
            .await
            .unwrap()
            .status(),
        400
    );
    w.shutdown().await.unwrap();
}

#[tokio::test]
async fn cancellation_deletion_and_disconnect_do_not_resubmit_or_publish_stale_results() {
    let h = Harness::new().await;
    let a = h.account("a@example.test").await;
    let c = id(&h.create(&a, "cancel").await, "id");
    let w = h.app.run_worker();
    let accepted = h.submit(&a, &c, turn("[blocked]", None)).await;
    let g = id(&accepted, "generation_id");
    h.calls(1).await;
    let r = h
        .client
        .get(format!("{}/v1/generations/{g}/events", h.base))
        .bearer_auth(&a)
        .send()
        .await
        .unwrap();
    let mut stream = r.bytes_stream();
    let first = stream.next().await.unwrap().unwrap();
    assert!(std::str::from_utf8(&first).unwrap().contains("accepted"));
    drop(stream);
    let cancelled = h
        .client
        .post(format!("{}/v1/generations/{g}/cancel", h.base))
        .bearer_auth(&a)
        .send()
        .await
        .unwrap();
    assert_eq!(cancelled.status(), 200);
    assert_eq!(h.terminal(&a, &g).await["status"], "cancelled");
    let replay = h.replay(&a, &g, 0).await;
    assert!(replay.contains("cancelled"));
    assert!(!replay.contains("completed"));
    assert_eq!(h.sim.requests.lock().unwrap().len(), 1);
    let history = h.get(&a, &format!("/v1/chats/{c}/messages")).await;
    assert_eq!(history["items"].as_array().unwrap().len(), 1);
    let queued = h.submit(&a, &c, turn("[blocked] second", None)).await;
    let g2 = id(&queued, "generation_id");
    h.calls(2).await;
    let chats = h.get(&a, "/v1/chats").await;
    let version = chats["items"][0]["version"].as_i64().unwrap();
    assert_eq!(
        h.client
            .delete(format!("{}/v1/chats/{c}?version={version}", h.base))
            .bearer_auth(&a)
            .send()
            .await
            .unwrap()
            .status(),
        204
    );
    assert_eq!(
        h.client
            .get(format!("{}/v1/generations/{g2}", h.base))
            .bearer_auth(&a)
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
    w.shutdown().await.unwrap();
}

#[tokio::test]
async fn private_validated_photos_vision_and_generated_images_are_durable() {
    let h = Harness::new().await;
    let a = h.account("a@example.test").await;
    let b = h.account("b@example.test").await;
    assert_eq!(
        h.client
            .post(format!("{}/v1/attachments", h.base))
            .bearer_auth(&a)
            .header("content-type", "image/png")
            .body("not a png")
            .send()
            .await
            .unwrap()
            .status(),
        400
    );
    let upload = h
        .client
        .post(format!("{}/v1/attachments", h.base))
        .bearer_auth(&a)
        .header("content-type", "image/png")
        .body(png())
        .send()
        .await
        .unwrap();
    assert_eq!(upload.status(), 201);
    let attachment: Value = upload.json().await.unwrap();
    let aid = id(&attachment, "id");
    assert_eq!(attachment["width"], 2);
    assert_eq!(
        h.client
            .get(format!("{}/v1/attachments/{aid}", h.base))
            .bearer_auth(&b)
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
    assert_eq!(
        h.client
            .delete(format!("{}/v1/attachments/{aid}", h.base))
            .bearer_auth(&b)
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
    let ca = id(&h.create(&a, "vision").await, "id");
    let cb = id(&h.create(&b, "other").await, "id");
    let payload = json!({"request_id":Uuid::new_v4(),"parent_id":null,"parts":[{"type":"text","text":"what is this?"},{"type":"image","attachment_id":aid}],"output":"text","replaces_message_id":null});
    assert_eq!(
        h.client
            .post(format!("{}/v1/chats/{cb}/turns", h.base))
            .bearer_auth(&b)
            .json(&payload)
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
    let w = h.app.run_worker();
    let accepted = h.submit(&a, &ca, payload).await;
    assert_eq!(
        h.terminal(&a, &id(&accepted, "generation_id")).await["status"],
        "completed"
    );
    assert!(
        h.sim.requests.lock().unwrap()[0]
            .to_string()
            .contains("data:image/png;base64,")
    );
    assert_eq!(
        h.client
            .delete(format!("{}/v1/attachments/{aid}", h.base))
            .bearer_auth(&a)
            .send()
            .await
            .unwrap()
            .status(),
        409
    );
    let mut generate = turn("a blue square", None);
    generate["output"] = json!("image");
    let accepted = h.submit(&a, &ca, generate).await;
    let status = h.terminal(&a, &id(&accepted, "generation_id")).await;
    assert_eq!(status["status"], "completed");
    let history = h.get(&a, &format!("/v1/chats/{ca}/messages")).await;
    let generated = id(&history["items"][0]["parts"][0], "attachment_id");
    let bytes = h
        .client
        .get(format!("{}/v1/attachments/{generated}", h.base))
        .bearer_auth(&a)
        .send()
        .await
        .unwrap();
    assert_eq!(bytes.status(), 200);
    assert_eq!(bytes.bytes().await.unwrap().as_ref(), png().as_slice());
    assert_eq!(h.sim.image_calls.load(Ordering::SeqCst), 1);
    let edit = json!({"request_id":Uuid::new_v4(),"parent_id":status["result_message_id"],"parts":[{"type":"text","text":"make the square green"},{"type":"image","attachment_id":generated},{"type":"image","attachment_id":aid}],"output":"image","replaces_message_id":null});
    let edited = h.submit(&a, &ca, edit).await;
    assert_eq!(
        h.terminal(&a, &id(&edited, "generation_id")).await["status"],
        "completed"
    );
    {
        use base64::Engine;
        let images = h.sim.image_requests.lock().unwrap();
        let expected = base64::engine::general_purpose::STANDARD.encode(png());
        assert_eq!(images[1]["input_image"], expected);
        assert_eq!(images[1]["input_image_2"], expected);
        assert_eq!(images[1]["prompt"], "make the square green\n");
    }
    assert_eq!(h.sim.image_calls.load(Ordering::SeqCst), 2);
    let version = h.get(&a, "/v1/chats").await["items"][0]["version"]
        .as_i64()
        .unwrap();
    assert_eq!(
        h.client
            .delete(format!("{}/v1/chats/{ca}?version={version}", h.base))
            .bearer_auth(&a)
            .send()
            .await
            .unwrap()
            .status(),
        204
    );
    assert_eq!(
        h.client
            .get(format!("{}/v1/attachments/{aid}", h.base))
            .bearer_auth(&a)
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
    assert_eq!(
        h.client
            .get(format!("{}/v1/attachments/{generated}", h.base))
            .bearer_auth(&a)
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
    w.shutdown().await.unwrap();
}

#[tokio::test]
async fn malformed_and_truncated_streams_fail_without_false_saved_answers() {
    let h = Harness::new().await;
    let a = h.account("a@example.test").await;
    let c = id(&h.create(&a, "bad streams").await, "id");
    let w = h.app.run_worker();
    let accepted = h.submit(&a, &c, turn("[truncated]", None)).await;
    let g = id(&accepted, "generation_id");
    assert_eq!(h.terminal(&a, &g).await["status"], "failed");
    assert!(!h.replay(&a, &g, 0).await.contains("completed"));
    let accepted = h.submit(&a, &c, turn("[malformed]", None)).await;
    let g = id(&accepted, "generation_id");
    assert_eq!(h.terminal(&a, &g).await["status"], "failed");
    let history = h.get(&a, &format!("/v1/chats/{c}/messages")).await;
    assert_eq!(history["items"].as_array().unwrap().len(), 1);
    assert_eq!(history["items"][0]["role"], "user");
    assert_eq!(h.sim.requests.lock().unwrap().len(), 2);
    w.shutdown().await.unwrap();
}

#[tokio::test]
async fn branch_page_snapshot_survives_a_branch_continuation() {
    let h = Harness::new().await;
    let a = h.account("snapshot@example.test").await;
    let c = id(&h.create(&a, "stable branches").await, "id");
    let w = h.app.run_worker();
    let old = h.submit(&a, &c, turn("older root", None)).await;
    let older_head = id(
        &h.terminal(&a, &id(&old, "generation_id")).await,
        "result_message_id",
    );
    let new = h.submit(&a, &c, turn("newer root", None)).await;
    let newer_head = id(
        &h.terminal(&a, &id(&new, "generation_id")).await,
        "result_message_id",
    );
    let first = h.get(&a, &format!("/v1/chats/{c}/branches?limit=1")).await;
    assert_eq!(first["items"][0]["id"], newer_head);
    let history = h
        .get(
            &a,
            &format!("/v1/chats/{c}/messages?head_id={older_head}&limit=1"),
        )
        .await;
    let next = h
        .submit(&a, &c, turn("continue older root", Some(&older_head)))
        .await;
    h.terminal(&a, &id(&next, "generation_id")).await;
    let second = h
        .get(
            &a,
            &format!(
                "/v1/chats/{c}/branches?limit=1&cursor={}",
                first["next_cursor"].as_str().unwrap()
            ),
        )
        .await;
    assert_eq!(
        second["items"].as_array().unwrap().len(),
        1,
        "a branch that existed at the snapshot must remain on its next page"
    );
    assert_eq!(second["items"][0]["id"], older_head);
    let older = h
        .get(
            &a,
            &format!(
                "/v1/chats/{c}/messages?limit=1&cursor={}",
                history["next_cursor"].as_str().unwrap()
            ),
        )
        .await;
    assert_eq!(older["items"][0]["parts"][0]["text"], "older root");
    assert_eq!(older["head_id"], older_head);
    w.shutdown().await.unwrap();
}

#[tokio::test]
async fn deleting_chats_reclaims_exclusive_images_and_preserves_shared_images() {
    let h = Harness::new().await;
    let a = h.account("storage@example.test").await;
    let upload = h
        .client
        .post(format!("{}/v1/attachments", h.base))
        .bearer_auth(&a)
        .header("content-type", "image/png")
        .body(png())
        .send()
        .await
        .unwrap();
    assert_eq!(upload.status(), 201);
    let attachment: Value = upload.json().await.unwrap();
    let aid = id(&attachment, "id");
    let first = id(&h.create(&a, "first").await, "id");
    let second = id(&h.create(&a, "second").await, "id");
    let w = h.app.run_worker();
    let mut first_turn = turn("shared image", None);
    first_turn["parts"]
        .as_array_mut()
        .unwrap()
        .push(json!({"type":"image","attachment_id":aid}));
    let accepted = h.submit(&a, &first, first_turn.clone()).await;
    h.terminal(&a, &id(&accepted, "generation_id")).await;
    first_turn["request_id"] = json!(Uuid::new_v4());
    let accepted = h.submit(&a, &second, first_turn).await;
    h.terminal(&a, &id(&accepted, "generation_id")).await;
    assert_eq!(
        h.client
            .delete(format!("{}/v1/chats/{first}?version=3", h.base))
            .bearer_auth(&a)
            .send()
            .await
            .unwrap()
            .status(),
        204
    );
    assert_eq!(
        h.client
            .get(format!("{}/v1/attachments/{aid}", h.base))
            .bearer_auth(&a)
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    assert_eq!(
        h.client
            .delete(format!("{}/v1/chats/{second}?version=3", h.base))
            .bearer_auth(&a)
            .send()
            .await
            .unwrap()
            .status(),
        204
    );
    assert_eq!(
        h.client
            .delete(format!("{}/v1/attachments/{aid}", h.base))
            .bearer_auth(&a)
            .send()
            .await
            .unwrap()
            .status(),
        404,
        "the last chat deletion must remove the exclusively retained image, not leave it undeletable forever"
    );
    w.shutdown().await.unwrap();
}
