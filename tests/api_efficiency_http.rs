//! Durable chat creation retries and whole-record byte-bounded pagination over real PostgreSQL.
use axum::{
    Json, Router,
    response::{IntoResponse, Response},
    routing::post,
};
use reqwest::{Client, Method};
use serde_json::{Value, json};
use smartcindy::{Application, Config, ProviderConfig};
use std::{collections::HashSet, time::Duration};
use testcontainers::{
    ContainerAsync, GenericImage, ImageExt,
    core::{IntoContainerPort, WaitFor},
    runners::AsyncRunner,
};
use uuid::Uuid;

async fn model(Json(body): Json<Value>) -> Response {
    if body["stream"] != true {
        return Json(json!({"choices":[{"finish_reason":"stop","message":{"content":"{\"summary\":\"Large text conversation.\",\"facts\":[]}"}}]})).into_response();
    }
    let answer = "reply".repeat(12000);
    let frame = json!({"choices":[{"delta":{"content":answer},"finish_reason":null}]}).to_string();
    ([("content-type","text/event-stream")],format!("data: {frame}\n\ndata: {{\"choices\":[{{\"delta\":{{}},\"finish_reason\":\"stop\"}}]}}\n\ndata: [DONE]\n\n")).into_response()
}

struct Harness {
    _db: ContainerAsync<GenericImage>,
    app: Application,
    client: Client,
    base: String,
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
    async fn new() -> Self {
        let db = GenericImage::new(
            "pgvector/pgvector",
            "pg18@sha256:ad249f9fb9572979643a47694e59b9a8cdcdf6abc6accebf49bb80ad5f20ff9b",
        )
        .with_exposed_port(5432.tcp())
        .with_wait_for(WaitFor::message_on_stderr(
            "listening on IPv4 address \"0.0.0.0\", port 5432",
        ))
        .with_env_var("POSTGRES_PASSWORD", "synthetic-efficiency-password")
        .start()
        .await
        .expect("Docker/PostgreSQL required; never skip");
        let url = format!(
            "postgres://postgres:synthetic-efficiency-password@127.0.0.1:{}/postgres",
            db.get_host_port_ipv4(5432.tcp()).await.unwrap()
        );
        Application::migrate(&url).await.unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let provider = tokio::spawn(async move {
            axum::serve(
                listener,
                Router::new().route("/chat/completions", post(model)),
            )
            .await
            .unwrap();
        });
        let mut config = Config::new(url);
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
            client: Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .unwrap(),
            base,
            server,
            provider,
        }
    }
    async fn account(&self, email: &str) -> String {
        let response=self.client.post(format!("{}/v1/accounts",self.base)).json(&json!({"email":email,"password":"synthetic-password-123","display_name":"Efficiency User"})).send().await.unwrap();
        assert_eq!(response.status(), 201);
        response.json::<Value>().await.unwrap()["token"]
            .as_str()
            .unwrap()
            .into()
    }
    async fn json(
        &self,
        method: Method,
        path: &str,
        token: &str,
        body: Value,
        status: u16,
    ) -> Value {
        let response = self
            .client
            .request(method, format!("{}{path}", self.base))
            .bearer_auth(token)
            .json(&body)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), status, "{path}");
        response.json().await.unwrap()
    }
    async fn delete(&self, path: &str, token: &str) {
        assert_eq!(
            self.client
                .delete(format!("{}{path}", self.base))
                .bearer_auth(token)
                .send()
                .await
                .unwrap()
                .status(),
            204
        );
    }
    async fn turn(&self, token: &str, chat: &str, parent: Option<&str>) -> (String, String) {
        let accepted=self.json(Method::POST,&format!("/v1/chats/{chat}/turns"),token,json!({"request_id":Uuid::new_v4(),"parent_id":parent,"parts":[{"type":"text","text":"user".repeat(8000)}],"output":"text"}),202).await;
        let generation = accepted["generation_id"].as_str().unwrap();
        let terminal = tokio::time::timeout(Duration::from_secs(15), async {
            loop {
                let state = self
                    .json(
                        Method::GET,
                        &format!("/v1/generations/{generation}"),
                        token,
                        Value::Null,
                        200,
                    )
                    .await;
                if state["status"] == "completed" {
                    break state;
                }
                assert!(
                    state["status"] == "queued" || state["status"] == "running",
                    "{state}"
                );
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        (
            accepted["user_message_id"].as_str().unwrap().into(),
            terminal["result_message_id"].as_str().unwrap().into(),
        )
    }
    async fn page(&self, token: &str, path: &str) -> Value {
        let response = self
            .client
            .get(format!("{}{path}", self.base))
            .bearer_auth(token)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        let bytes = response.bytes().await.unwrap();
        assert!(
            bytes.len() <= 512 * 1024,
            "response exceeded byte budget: {}",
            bytes.len()
        );
        serde_json::from_slice(&bytes).unwrap()
    }
}

#[tokio::test]
async fn chat_create_retries_are_owner_scoped_and_survive_project_changes_without_reviving_deletion()
 {
    let h = Harness::new().await;
    let owner = h.account("create-owner@example.test").await;
    let other = h.account("create-other@example.test").await;
    let request = Uuid::new_v4();
    let payload = json!({"request_id":request,"title":"Original title"});
    let endpoint = format!("{}/v1/chats", h.base);
    let (a, b) = tokio::join!(
        h.client
            .post(&endpoint)
            .bearer_auth(&owner)
            .json(&payload)
            .send(),
        h.client
            .post(&endpoint)
            .bearer_auth(&owner)
            .json(&payload)
            .send()
    );
    let a = a.unwrap();
    let b = b.unwrap();
    let mut statuses = [a.status().as_u16(), b.status().as_u16()];
    statuses.sort();
    assert_eq!(statuses, [200, 201]);
    let a: Value = a.json().await.unwrap();
    let b: Value = b.json().await.unwrap();
    assert_eq!(a["id"], b["id"]);
    let id = a["id"].as_str().unwrap();
    h.json(
        Method::POST,
        "/v1/chats",
        &owner,
        json!({"request_id":request,"title":"Changed title"}),
        409,
    )
    .await;
    let different = h
        .json(Method::POST, "/v1/chats", &other, payload.clone(), 201)
        .await;
    assert_ne!(different["id"], a["id"]);
    h.json(
        Method::PATCH,
        &format!("/v1/chats/{id}"),
        &owner,
        json!({"version":a["version"],"title":"Renamed later"}),
        200,
    )
    .await;
    let replay = h
        .json(Method::POST, "/v1/chats", &owner, payload.clone(), 200)
        .await;
    assert_eq!(replay["id"], a["id"]);
    assert_eq!(replay["title"], "Renamed later");
    h.delete(
        &format!("/v1/chats/{id}?version={}", replay["version"]),
        &owner,
    )
    .await;
    h.json(Method::POST, "/v1/chats", &owner, payload, 410)
        .await;
    let project = h
        .json(
            Method::POST,
            "/v1/projects",
            &owner,
            json!({"request_id":Uuid::new_v4(),"name":"Trip"}),
            201,
        )
        .await;
    let project_id = project["id"].as_str().unwrap();
    let grouped = json!({"request_id":Uuid::new_v4(),"title":"Trip plans","project_id":project_id});
    let chat = h
        .json(Method::POST, "/v1/chats", &owner, grouped.clone(), 201)
        .await;
    h.delete(
        &format!("/v1/projects/{project_id}?version={}", project["version"]),
        &owner,
    )
    .await;
    let replay = h
        .json(Method::POST, "/v1/chats", &owner, grouped, 200)
        .await;
    assert_eq!(replay["id"], chat["id"]);
    assert!(replay["project_id"].is_null());
    let legacy_a = h
        .json(
            Method::POST,
            "/v1/chats",
            &owner,
            json!({"title":"legacy"}),
            201,
        )
        .await;
    let legacy_b = h
        .json(
            Method::POST,
            "/v1/chats",
            &owner,
            json!({"title":"legacy"}),
            201,
        )
        .await;
    assert_ne!(legacy_a["id"], legacy_b["id"]);
}

#[tokio::test]
async fn large_history_and_branch_pages_are_byte_bounded_without_missing_or_duplicating_records() {
    let h = Harness::new().await;
    let owner = h.account("pages-owner@example.test").await;
    h.json(
        Method::PATCH,
        "/v1/memory/settings",
        &owner,
        json!({"version":0,"enabled":false}),
        200,
    )
    .await;
    let chat = h
        .json(
            Method::POST,
            "/v1/chats",
            &owner,
            json!({"title":"long history"}),
            201,
        )
        .await;
    let id = chat["id"].as_str().unwrap();
    let worker = h.app.run_worker();
    let mut head = None;
    let mut expected = Vec::new();
    for _ in 0..12 {
        let (user, assistant) = h.turn(&owner, id, head.as_deref()).await;
        expected.push(user);
        expected.push(assistant.clone());
        head = Some(assistant);
    }
    expected.reverse();
    let mut path = format!("/v1/chats/{id}/messages?limit=100");
    let mut seen = Vec::new();
    let mut pages = 0;
    loop {
        let page = h.page(&owner, &path).await;
        pages += 1;
        let items = page["items"].as_array().unwrap();
        assert!(!items.is_empty());
        assert_eq!(page["head_id"], json!(head));
        for item in items {
            let content = item["parts"][0]["text"].as_str().unwrap();
            assert!(
                content.len() == 32000 || content.len() == 60000,
                "whole record preserved"
            );
            seen.push(item["id"].as_str().unwrap().to_owned());
        }
        match page["next_cursor"].as_str() {
            Some(cursor) => path = format!("/v1/chats/{id}/messages?limit=100&cursor={cursor}"),
            None => break,
        }
        assert!(pages < 10, "pagination must make progress");
    }
    assert!(pages > 1);
    assert_eq!(seen, expected);
    // Seeded public property: count limits and the byte limit cannot alter traversal.
    use proptest::{
        strategy::{Strategy, ValueTree},
        test_runner::{Config as PropertyConfig, RngSeed, TestRunner},
    };
    let mut runner = TestRunner::new(PropertyConfig {
        cases: 4,
        rng_seed: RngSeed::Fixed(73641989),
        ..PropertyConfig::default()
    });
    for _ in 0..4 {
        let limit = (1usize..=100).new_tree(&mut runner).unwrap().current();
        let mut path = format!("/v1/chats/{id}/messages?limit={limit}");
        let mut seen = Vec::new();
        loop {
            let page = h.page(&owner, &path).await;
            let items = page["items"].as_array().unwrap();
            assert!(!items.is_empty() && items.len() <= limit);
            seen.extend(
                items
                    .iter()
                    .map(|item| item["id"].as_str().unwrap().to_owned()),
            );
            assert!(seen.len() <= 24);
            match page["next_cursor"].as_str() {
                Some(cursor) => {
                    path = format!("/v1/chats/{id}/messages?limit={limit}&cursor={cursor}")
                }
                None => break,
            }
        }
        assert_eq!(seen, expected);
    }
    let roots = h
        .json(
            Method::POST,
            "/v1/chats",
            &owner,
            json!({"title":"large branches"}),
            201,
        )
        .await;
    let roots_id = roots["id"].as_str().unwrap();
    let mut expected_heads = Vec::new();
    for _ in 0..12 {
        let (_, head) = h.turn(&owner, roots_id, None).await;
        expected_heads.push(head);
    }
    expected_heads.reverse();
    let first = h
        .page(&owner, &format!("/v1/chats/{roots_id}/branches?limit=100"))
        .await;
    assert!(first["next_cursor"].is_string());
    // A snapshot must preserve a branch that continues while its older pages are loaded.
    let oldest = expected_heads.last().unwrap();
    h.turn(&owner, roots_id, Some(oldest)).await;
    let mut seen: Vec<String> = first["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["id"].as_str().unwrap().into())
        .collect();
    let mut cursor = first["next_cursor"].as_str().map(str::to_owned);
    while let Some(next) = cursor {
        let page = h
            .page(
                &owner,
                &format!("/v1/chats/{roots_id}/branches?limit=100&cursor={next}"),
            )
            .await;
        seen.extend(
            page["items"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v["id"].as_str().unwrap().to_owned()),
        );
        cursor = page["next_cursor"].as_str().map(str::to_owned);
        assert!(seen.len() <= 12);
    }
    assert_eq!(seen, expected_heads);
    assert_eq!(seen.iter().collect::<HashSet<_>>().len(), 12);
    worker.shutdown().await.unwrap();
}
