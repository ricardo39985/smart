//! Memory-aware replies through public HTTP against real PostgreSQL/pgvector.
//! The paid model is a protocol simulator: these assert supplied evidence, not AI quality.
use axum::{
    Json, Router,
    extract::State,
    response::{
        IntoResponse, Response,
        sse::{Event, Sse},
    },
    routing::post,
};
use serde_json::{Value, json};
use smartcindy::{Application, Config, ProviderConfig};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};
use testcontainers::{
    ContainerAsync, GenericImage, ImageExt,
    core::{IntoContainerPort, WaitFor},
    runners::AsyncRunner,
};
use tokio::sync::Notify;
use uuid::Uuid;

#[derive(Clone, Default)]
struct Model {
    replies: Arc<AtomicUsize>,
    extractions: Arc<AtomicUsize>,
    hold_extraction: Arc<AtomicBool>,
    extraction_entered: Arc<Notify>,
    extraction_release: Arc<Notify>,
}

async fn model(State(model): State<Model>, Json(body): Json<Value>) -> Response {
    if body["stream"] != true {
        model.extractions.fetch_add(1, Ordering::SeqCst);
        let input: Value = body["messages"]
            .as_array()
            .unwrap()
            .iter()
            .rev()
            .find_map(|message| {
                message["content"]
                    .as_str()
                    .and_then(|text| serde_json::from_str(text).ok())
            })
            .expect("background input is structured source-attributed text");
        let corrected_ids: Vec<Value> = input["existing_memories"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|memory| memory["content"] == "My favorite tea is jasmine.")
            .map(|memory| memory["id"].clone())
            .collect();
        let facts: Vec<Value> = input["messages"].as_array().unwrap().iter()
            .filter(|message| message["role"] == "user" && message["fact_eligible"] == true && message["text"].as_str().unwrap().contains("My favorite tea is "))
            .map(|message| {
                if message["text"].as_str().unwrap().contains("My favorite tea is mint.") {
                    json!({"source_message_id":message["id"],"quote":"My favorite tea is mint.","supersedes":corrected_ids})
                } else {
                    json!({"source_message_id":message["id"],"quote":"My favorite tea is jasmine."})
                }
            }).collect();
        model.extraction_entered.notify_one();
        if model.hold_extraction.load(Ordering::SeqCst) {
            model.extraction_release.notified().await;
        }
        return Json(json!({"choices":[{"finish_reason":"stop","message":{"content":json!({"summary":"The conversation covered routine planning.","facts":facts}).to_string()}}],"usage":{"prompt_tokens":100,"completion_tokens":10,"total_tokens":110,"prompt_cache_hit_tokens":80,"prompt_cache_miss_tokens":20}})).into_response();
    }
    model.replies.fetch_add(1, Ordering::SeqCst);
    let supplied = body["messages"].to_string();
    let evidence = json!({
        "personality":body["messages"][0]["role"] == "system" && body["messages"][0]["content"].as_str().unwrap_or_default().contains("Cindy"),
        "zephyr":supplied.contains("Zephyr observatory"),
        "aurora":supplied.contains("Aurora observatory"),
        "launch":supplied.contains("launch code is ORCHID"),
        "jasmine":supplied.contains("My favorite tea is jasmine."),
        "mint":supplied.contains("My favorite tea is mint.")
    }).to_string();
    let stream = async_stream::stream! {
        yield Ok::<_, std::convert::Infallible>(Event::default().data(json!({"choices":[{"delta":{"content":evidence},"finish_reason":null}]}).to_string()));
        yield Ok(Event::default().data(json!({"choices":[{"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":100,"completion_tokens":10,"total_tokens":110,"prompt_cache_hit_tokens":80,"prompt_cache_miss_tokens":20}}).to_string()));
        yield Ok(Event::default().data("[DONE]"));
    };
    Sse::new(stream).into_response()
}

struct Harness {
    _db: ContainerAsync<GenericImage>,
    app: Application,
    base: String,
    client: reqwest::Client,
    model: Model,
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
        let db = GenericImage::new(
            "pgvector/pgvector",
            "pg18@sha256:ad249f9fb9572979643a47694e59b9a8cdcdf6abc6accebf49bb80ad5f20ff9b",
        )
        .with_exposed_port(5432.tcp())
        .with_wait_for(WaitFor::message_on_stderr(
            "listening on IPv4 address \"0.0.0.0\", port 5432",
        ))
        .with_env_var("POSTGRES_PASSWORD", "synthetic-only-password")
        .start()
        .await
        .expect("Docker/PostgreSQL required; do not skip");
        let database_url = format!(
            "postgres://postgres:synthetic-only-password@127.0.0.1:{}/postgres",
            db.get_host_port_ipv4(5432.tcp()).await.unwrap()
        );
        Application::migrate(&database_url).await.unwrap();
        let model_state = Model::default();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let routes = Router::new()
            .route("/chat/completions", post(model))
            .with_state(model_state.clone());
        let simulator = tokio::spawn(async move {
            axum::serve(listener, routes).await.unwrap();
        });
        let mut config = Config::new(database_url);
        config.providers = ProviderConfig {
            deepseek_key: Some("synthetic-key".into()),
            bfl_key: None,
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
            app,
            base,
            model: model_state,
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(15))
                .build()
                .unwrap(),
            server,
            simulator,
        }
    }
    async fn account(&self, email: &str) -> String {
        let response = self.client.post(format!("{}/v1/accounts",self.base)).json(&json!({"email":email,"password":"synthetic-password-123","display_name":"Context User"})).send().await.unwrap();
        assert_eq!(response.status(), 201);
        response.json::<Value>().await.unwrap()["token"]
            .as_str()
            .unwrap()
            .into()
    }
    async fn get(&self, token: &str, path: &str) -> Value {
        let response = self
            .client
            .get(format!("{}{path}", self.base))
            .bearer_auth(token)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        response.json().await.unwrap()
    }
    async fn chat(&self, token: &str) -> String {
        let response = self
            .client
            .post(format!("{}/v1/chats", self.base))
            .bearer_auth(token)
            .json(&json!({"title":"Memory journey"}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 201);
        response.json::<Value>().await.unwrap()["id"]
            .as_str()
            .unwrap()
            .into()
    }
    async fn submit(&self, token: &str, chat: &str, payload: &Value) -> Value {
        let response = self
            .client
            .post(format!("{}/v1/chats/{chat}/turns", self.base))
            .bearer_auth(token)
            .json(payload)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 202);
        response.json().await.unwrap()
    }
    async fn complete(&self, token: &str, generation: &str) -> Value {
        tokio::time::timeout(Duration::from_secs(15), async {
            loop {
                let value = self
                    .get(token, &format!("/v1/generations/{generation}"))
                    .await;
                if !matches!(value["status"].as_str(), Some("queued" | "running")) {
                    assert_eq!(value["status"], "completed", "{value}");
                    return value;
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        })
        .await
        .expect("generation completes within bounded deadline")
    }
    async fn answer(
        &self,
        token: &str,
        chat: &str,
        text: &str,
        parent: Option<&str>,
    ) -> (Value, Value) {
        let accepted = self.submit(token, chat, &turn(text, parent)).await;
        let result = self
            .complete(token, accepted["generation_id"].as_str().unwrap())
            .await;
        let history = self
            .get(
                token,
                &format!(
                    "/v1/chats/{chat}/messages?head_id={}&limit=1",
                    result["result_message_id"].as_str().unwrap()
                ),
            )
            .await;
        let evidence =
            serde_json::from_str(history["items"][0]["parts"][0]["text"].as_str().unwrap())
                .unwrap();
        (result, evidence)
    }
    async fn memory(&self, token: &str, content: &str) -> Value {
        let response = self
            .client
            .post(format!("{}/v1/memories", self.base))
            .bearer_auth(token)
            .json(&json!({"request_id":Uuid::new_v4(),"content":content}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 201);
        response.json().await.unwrap()
    }
    async fn finished_memory_jobs(&self, token: &str, count: usize) -> Value {
        tokio::time::timeout(Duration::from_secs(15), async {
            loop {
                let jobs = self.get(token, "/v1/memory/jobs?limit=100").await;
                let items = jobs["items"].as_array().unwrap();
                if items.len() == count
                    && items
                        .iter()
                        .all(|job| !matches!(job["status"].as_str(), Some("queued" | "running")))
                {
                    return jobs;
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        })
        .await
        .expect("background jobs reach their public terminal states")
    }
}
fn turn(text: &str, parent: Option<&str>) -> Value {
    json!({"request_id":Uuid::new_v4(),"parent_id":parent,"parts":[{"type":"text","text":text}],"output":"text","replaces_message_id":null})
}

#[tokio::test]
async fn corrected_deleted_and_disabled_memories_change_the_next_reply_without_crossing_accounts() {
    let h = Harness::new().await;
    let owner = h.account("context-owner@example.test").await;
    let other = h.account("context-other@example.test").await;
    let memory = h
        .memory(&owner, "My favorite museum is the Zephyr observatory.")
        .await;
    let worker = h.app.run_worker();
    let c = h.chat(&owner).await;
    let (done, evidence) = h
        .answer(&owner, &c, "Which observatory do I prefer?", None)
        .await;
    assert_eq!(
        evidence,
        json!({"personality":true,"zephyr":true,"aurora":false,"launch":false,"jasmine":false,"mint":false})
    );
    assert_eq!(done["usage"]["cached_input_tokens"], 80);
    assert_eq!(done["usage"]["uncached_input_tokens"], 20);
    let c = h.chat(&other).await;
    let (_, evidence) = h
        .answer(&other, &c, "Which observatory do I prefer?", None)
        .await;
    assert_eq!(evidence["zephyr"], false);

    let response = h.client.patch(format!("{}/v1/memories/{}",h.base,memory["id"].as_str().unwrap())).bearer_auth(&owner).json(&json!({"version":memory["version"],"content":"My favorite museum is the Aurora observatory."})).send().await.unwrap();
    assert_eq!(response.status(), 200);
    let corrected: Value = response.json().await.unwrap();
    let c = h.chat(&owner).await;
    let (_, evidence) = h
        .answer(&owner, &c, "Which observatory do I prefer?", None)
        .await;
    assert_eq!(evidence["aurora"], true);
    assert_eq!(evidence["zephyr"], false);

    let response = h
        .client
        .delete(format!(
            "{}/v1/memories/{}?version={}",
            h.base,
            corrected["id"].as_str().unwrap(),
            corrected["version"]
        ))
        .bearer_auth(&owner)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 204);
    let c = h.chat(&owner).await;
    let (_, evidence) = h
        .answer(&owner, &c, "Which observatory do I prefer?", None)
        .await;
    assert_eq!(evidence["aurora"], false);
    h.memory(&owner, "My favorite museum is the Zephyr observatory.")
        .await;
    let settings = h.get(&owner, "/v1/memory/settings").await;
    let response = h
        .client
        .patch(format!("{}/v1/memory/settings", h.base))
        .bearer_auth(&owner)
        .json(&json!({"version":settings["version"],"enabled":false}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let c = h.chat(&owner).await;
    let payload = turn("Which observatory do I prefer?", None);
    let accepted = h.submit(&owner, &c, &payload).await;
    let done = h
        .complete(&owner, accepted["generation_id"].as_str().unwrap())
        .await;
    let history = h
        .get(&owner, &format!("/v1/chats/{c}/messages?limit=1"))
        .await;
    let evidence: Value =
        serde_json::from_str(history["items"][0]["parts"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(evidence["zephyr"], false);
    let before = h.model.replies.load(Ordering::SeqCst);
    let repeated = h.submit(&owner, &c, &payload).await;
    assert_eq!(repeated["generation_id"], accepted["generation_id"]);
    assert_eq!(
        h.complete(&owner, repeated["generation_id"].as_str().unwrap())
            .await,
        done
    );
    assert_eq!(
        h.model.replies.load(Ordering::SeqCst),
        before,
        "repeating an accepted request must not buy another generation"
    );
    worker.shutdown().await.unwrap();
}

#[tokio::test]
async fn returning_to_an_older_topic_recalls_its_source_but_an_alternate_branch_does_not() {
    let h = Harness::new().await;
    let owner = h.account("topics@example.test").await;
    let worker = h.app.run_worker();
    let c = h.chat(&owner).await;
    let (root, _) = h
        .answer(&owner, &c, "Start a planning conversation.", None)
        .await;
    let branch_point = root["result_message_id"].as_str().unwrap().to_owned();
    let (first, _) = h
        .answer(
            &owner,
            &c,
            "For the Zephyr project, the launch code is ORCHID.",
            Some(&branch_point),
        )
        .await;
    let mut parent = first["result_message_id"].as_str().unwrap().to_owned();
    for _ in 0..6 {
        let (done, _) = h
            .answer(
                &owner,
                &c,
                "Let us discuss garden tools and watering schedules.",
                Some(&parent),
            )
            .await;
        parent = done["result_message_id"].as_str().unwrap().to_owned();
    }
    // The first completed job followed indexing the oldest source batch, which
    // includes the target fact (the third chronological message).
    let jobs = h.finished_memory_jobs(&owner, 1).await;
    assert_eq!(jobs["items"][0]["status"], "completed");
    for _ in 0..14 {
        let (done, _) = h
            .answer(
                &owner,
                &c,
                "Let us discuss garden tools and watering schedules.",
                Some(&parent),
            )
            .await;
        parent = done["result_message_id"].as_str().unwrap().to_owned();
    }
    let (_, evidence) = h
        .answer(
            &owner,
            &c,
            "Back to the Zephyr project: what was its launch code?",
            Some(&parent),
        )
        .await;
    assert_eq!(
        evidence["launch"], true,
        "an older topic must be recalled beyond the recent-message window"
    );
    let (_, evidence) = h
        .answer(
            &owner,
            &c,
            "What is the Zephyr project's launch code?",
            Some(&branch_point),
        )
        .await;
    assert_eq!(
        evidence["launch"], false,
        "an alternate branch must not inherit another branch's passages"
    );
    worker.shutdown().await.unwrap();
}

#[tokio::test]
async fn disabling_memory_during_extraction_prevents_late_facts_from_returning() {
    let h = Harness::new().await;
    let owner = h.account("extraction-fence@example.test").await;
    h.model.hold_extraction.store(true, Ordering::SeqCst);
    let worker = h.app.run_worker();
    let c = h.chat(&owner).await;
    let (first, _) = h
        .answer(&owner, &c, "My favorite tea is jasmine.", None)
        .await;
    let mut parent = first["result_message_id"].as_str().unwrap().to_owned();
    for _ in 0..7 {
        let (done, _) = h
            .answer(
                &owner,
                &c,
                "Let us plan the weekly shopping.",
                Some(&parent),
            )
            .await;
        parent = done["result_message_id"].as_str().unwrap().to_owned();
    }
    tokio::time::timeout(
        Duration::from_secs(12),
        h.model.extraction_entered.notified(),
    )
    .await
    .expect("background extraction starts");
    let settings = h.get(&owner, "/v1/memory/settings").await;
    let response = h
        .client
        .patch(format!("{}/v1/memory/settings", h.base))
        .bearer_auth(&owner)
        .json(&json!({"version":settings["version"],"enabled":false}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let disabled: Value = response.json().await.unwrap();
    let response = h
        .client
        .patch(format!("{}/v1/memory/settings", h.base))
        .bearer_auth(&owner)
        .json(&json!({"version":disabled["version"],"enabled":true}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    h.model.hold_extraction.store(false, Ordering::SeqCst);
    h.model.extraction_release.notify_one();
    for _ in 0..8 {
        let (done, _) = h
            .answer(
                &owner,
                &c,
                "Let us plan the weekly shopping.",
                Some(&parent),
            )
            .await;
        parent = done["result_message_id"].as_str().unwrap().to_owned();
    }
    // Wait for actual public completion, not merely receipt at the simulator.
    let jobs = h.finished_memory_jobs(&owner, 2).await;
    assert_eq!(jobs["items"][0]["status"], "completed");
    assert_eq!(jobs["items"][1]["status"], "cancelled");
    assert!(
        h.get(&owner, "/v1/memories").await["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let c = h.chat(&owner).await;
    let (_, evidence) = h.answer(&owner, &c, "What is my favorite tea?", None).await;
    assert_eq!(evidence["jasmine"], false);
    assert_eq!(
        h.model.extractions.load(Ordering::SeqCst),
        2,
        "there is one extraction for each eight-turn batch, without paid resubmission"
    );
    worker.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_new_explicit_fact_supersedes_an_older_derived_memory_in_the_next_batch() {
    let h = Harness::new().await;
    let owner = h.account("derived-correction@example.test").await;
    let worker = h.app.run_worker();
    let c = h.chat(&owner).await;
    let (first, _) = h
        .answer(&owner, &c, "My favorite tea is jasmine.", None)
        .await;
    let mut parent = first["result_message_id"].as_str().unwrap().to_owned();
    for _ in 0..7 {
        let (done, _) = h
            .answer(
                &owner,
                &c,
                "Let us plan the weekly shopping.",
                Some(&parent),
            )
            .await;
        parent = done["result_message_id"].as_str().unwrap().to_owned();
    }
    let jobs = h.finished_memory_jobs(&owner, 1).await;
    assert_eq!(jobs["items"][0]["status"], "completed");
    let memories = h.get(&owner, "/v1/memories").await;
    assert_eq!(memories["items"].as_array().unwrap().len(), 1);
    assert_eq!(
        memories["items"][0]["content"],
        "My favorite tea is jasmine."
    );
    let (corrected, _) = h
        .answer(&owner, &c, "My favorite tea is mint.", Some(&parent))
        .await;
    parent = corrected["result_message_id"].as_str().unwrap().to_owned();
    for _ in 0..7 {
        let (done, _) = h
            .answer(
                &owner,
                &c,
                "Let us plan the weekly shopping.",
                Some(&parent),
            )
            .await;
        parent = done["result_message_id"].as_str().unwrap().to_owned();
    }
    let jobs = h.finished_memory_jobs(&owner, 2).await;
    assert_eq!(jobs["items"][0]["status"], "completed");
    let memories = h.get(&owner, "/v1/memories").await;
    assert_eq!(memories["items"].as_array().unwrap().len(), 1);
    assert_eq!(memories["items"][0]["content"], "My favorite tea is mint.");
    let c = h.chat(&owner).await;
    let (_, evidence) = h.answer(&owner, &c, "What is my favorite tea?", None).await;
    assert_eq!(evidence["mint"], true);
    assert_eq!(evidence["jasmine"], false);
    worker.shutdown().await.unwrap();
}
