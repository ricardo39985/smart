//! Project ownership, versions and non-destructive deletion through the public HTTP API.
use axum::{Router, routing::post};
use proptest::strategy::{Strategy, ValueTree};
use reqwest::{Client, Method};
use serde_json::{Value, json};
use smartcindy::{Application, Config, ProviderConfig};
use std::time::Duration;
use testcontainers::{
    ContainerAsync, GenericImage, ImageExt,
    core::{IntoContainerPort, WaitFor},
    runners::AsyncRunner,
};
use uuid::Uuid;

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
        .with_env_var("POSTGRES_PASSWORD", "synthetic-project-password")
        .start()
        .await
        .expect("Docker/PostgreSQL required; never skip");
        let url = format!(
            "postgres://postgres:synthetic-project-password@127.0.0.1:{}/postgres",
            db.get_host_port_ipv4(5432.tcp()).await.unwrap()
        );
        Application::migrate(&url).await.unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let provider_url = format!("http://{}", listener.local_addr().unwrap());
        let provider = tokio::spawn(async move {
            axum::serve(listener, Router::new().route("/chat/completions", post(|| async {
                ([("content-type", "text/event-stream")], "data: {\"choices\":[{\"delta\":{\"content\":\"Preserved answer.\"},\"finish_reason\":null}]}\n\ndata: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n")
            }))).await.unwrap();
        });
        let mut config = Config::new(url.clone());
        config.providers = ProviderConfig {
            deepseek_key: Some("synthetic-key".into()),
            bfl_key: None,
            local_simulator: Some(provider_url),
        };
        let app = Application::new(sqlx::PgPool::connect(&url).await.unwrap(), config)
            .await
            .unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server_app = app.clone();
        let server = tokio::spawn(async move {
            axum::serve(listener, server_app.router()).await.unwrap();
        });
        let client = Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap();
        Self {
            _db: db,
            app,
            client,
            base,
            server,
            provider,
        }
    }

    async fn signup(&self, email: &str) -> String {
        let response = self.client.post(format!("{}/v1/accounts", self.base))
            .json(&json!({"email":email,"password":"synthetic-password-123","display_name":"Projects User"}))
            .send().await.unwrap();
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
            .request(method, format!("{}{}", self.base, path))
            .bearer_auth(token)
            .json(&body)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), status, "{path}");
        response.json().await.unwrap()
    }

    async fn status(&self, method: Method, path: &str, token: &str, body: Value, status: u16) {
        let response = self
            .client
            .request(method, format!("{}{}", self.base, path))
            .bearer_auth(token)
            .json(&body)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), status, "{path}");
    }
}

#[tokio::test]
async fn projects_keep_chat_history_after_moves_and_deletion() {
    let h = Harness::new().await;
    let owner = h.signup("project-owner@example.com").await;
    let other = h.signup("project-other@example.com").await;
    h.status(Method::GET, "/v1/projects", "", Value::Null, 401)
        .await;

    let request_id = Uuid::new_v4();
    let payload = json!({"request_id":request_id,"name":"Portugal trip"});
    let project = h
        .json(Method::POST, "/v1/projects", &owner, payload.clone(), 201)
        .await;
    let id = project["id"].as_str().unwrap();
    let path = format!("/v1/projects/{id}");
    assert_eq!(project["version"], 0);
    assert_eq!(
        h.json(Method::POST, "/v1/projects", &owner, payload.clone(), 200)
            .await["id"],
        project["id"]
    );
    h.status(
        Method::POST,
        "/v1/projects",
        &owner,
        json!({"request_id":request_id,"name":"different"}),
        409,
    )
    .await;
    h.status(Method::GET, &path, &other, Value::Null, 404).await;
    h.status(
        Method::PATCH,
        &path,
        &other,
        json!({"version":0,"name":"stolen"}),
        404,
    )
    .await;
    h.status(
        Method::DELETE,
        &format!("{path}?version=0"),
        &other,
        Value::Null,
        404,
    )
    .await;
    assert_eq!(
        h.json(Method::GET, "/v1/projects", &other, Value::Null, 200)
            .await["items"],
        json!([])
    );

    let renamed = h
        .json(
            Method::PATCH,
            &path,
            &owner,
            json!({"version":0,"name":"Lisbon trip"}),
            200,
        )
        .await;
    assert_eq!(renamed["version"], 1);
    assert_eq!(renamed["name"], "Lisbon trip");
    h.status(
        Method::PATCH,
        &path,
        &owner,
        json!({"version":0,"name":"stale"}),
        409,
    )
    .await;
    h.status(
        Method::DELETE,
        &format!("{path}?version=0"),
        &owner,
        Value::Null,
        409,
    )
    .await;

    let chat = h
        .json(
            Method::POST,
            "/v1/chats",
            &owner,
            json!({"title":"Hotel notes","project_id":id}),
            201,
        )
        .await;
    let chat_id = chat["id"].as_str().unwrap();
    let chat_path = format!("/v1/chats/{chat_id}");
    let move_path = format!("{chat_path}/project");
    assert_eq!(chat["project_id"], project["id"]);
    h.status(
        Method::POST,
        "/v1/chats",
        &other,
        json!({"title":"bad","project_id":id}),
        404,
    )
    .await;
    h.status(
        Method::POST,
        &move_path,
        &other,
        json!({"version":1,"project_id":null}),
        404,
    )
    .await;
    h.status(Method::POST, &move_path, &owner, json!({"version":1}), 400)
        .await;
    let detached = h
        .json(
            Method::POST,
            &move_path,
            &owner,
            json!({"version":1,"project_id":null}),
            200,
        )
        .await;
    assert_eq!(detached["project_id"], Value::Null);
    assert_eq!(detached["version"], 2);
    h.status(
        Method::POST,
        &move_path,
        &owner,
        json!({"version":1,"project_id":id}),
        409,
    )
    .await;
    let foreign = h
        .json(
            Method::POST,
            "/v1/projects",
            &other,
            json!({"request_id":request_id,"name":"Other project"}),
            201,
        )
        .await;
    h.status(
        Method::POST,
        &move_path,
        &owner,
        json!({"version":2,"project_id":foreign["id"]}),
        404,
    )
    .await;
    h.json(
        Method::POST,
        &move_path,
        &owner,
        json!({"version":2,"project_id":id}),
        200,
    )
    .await;
    assert_eq!(
        h.json(
            Method::GET,
            &format!("/v1/chats?project_id={id}"),
            &owner,
            Value::Null,
            200
        )
        .await["items"][0]["id"],
        chat["id"]
    );

    let accepted = h.json(Method::POST, &format!("{chat_path}/turns"), &owner,
        json!({"request_id":Uuid::new_v4(),"parent_id":null,"parts":[{"type":"text","text":"Keep these hotel notes."}],"output":"text","replaces_message_id":null}), 202).await;
    let worker = h.app.run_worker();
    let generation = accepted["generation_id"].as_str().unwrap();
    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            let state = h
                .json(
                    Method::GET,
                    &format!("/v1/generations/{generation}"),
                    &owner,
                    Value::Null,
                    200,
                )
                .await;
            if state["status"] == "completed" {
                break;
            }
            assert_ne!(state["status"], "failed");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    worker.shutdown().await.unwrap();
    let before = h
        .json(Method::GET, &chat_path, &owner, Value::Null, 200)
        .await;
    let history = h
        .json(
            Method::GET,
            &format!("{chat_path}/messages"),
            &owner,
            Value::Null,
            200,
        )
        .await;
    assert_eq!(history["items"].as_array().unwrap().len(), 2);
    h.status(
        Method::DELETE,
        &format!("{path}?version=1"),
        &owner,
        Value::Null,
        204,
    )
    .await;
    h.status(Method::GET, &path, &owner, Value::Null, 404).await;
    h.status(Method::POST, "/v1/projects", &owner, payload, 410)
        .await;
    let after = h
        .json(Method::GET, &chat_path, &owner, Value::Null, 200)
        .await;
    assert_eq!(after["title"], "Hotel notes");
    assert_eq!(after["project_id"], Value::Null);
    assert_eq!(
        after["version"].as_i64().unwrap(),
        before["version"].as_i64().unwrap() + 1
    );
    assert_eq!(
        h.json(
            Method::GET,
            &format!("{chat_path}/messages"),
            &owner,
            Value::Null,
            200
        )
        .await["items"],
        history["items"]
    );
    h.status(
        Method::POST,
        "/v1/chats",
        &owner,
        json!({"title":"bad","project_id":id}),
        404,
    )
    .await;
    h.status(
        Method::POST,
        &move_path,
        &owner,
        json!({"version":after["version"],"project_id":id}),
        404,
    )
    .await;
}

#[tokio::test]
async fn project_creation_is_idempotent_and_pages_are_bounded_and_owner_scoped() {
    let h = Harness::new().await;
    let owner = h.signup("project-pages@example.com").await;
    let other = h.signup("project-pages-other@example.com").await;
    let payload = json!({"request_id":Uuid::new_v4(),"name":"First"});
    let url = format!("{}/v1/projects", h.base);
    let (a, b) = tokio::join!(
        h.client
            .post(&url)
            .bearer_auth(&owner)
            .json(&payload)
            .send(),
        h.client
            .post(&url)
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
    let second = h
        .json(
            Method::POST,
            "/v1/projects",
            &owner,
            json!({"request_id":Uuid::new_v4(),"name":"Second"}),
            201,
        )
        .await;
    let third = h
        .json(
            Method::POST,
            "/v1/projects",
            &owner,
            json!({"request_id":Uuid::new_v4(),"name":"Third"}),
            201,
        )
        .await;
    let page = h
        .json(
            Method::GET,
            "/v1/projects?limit=2",
            &owner,
            Value::Null,
            200,
        )
        .await;
    assert_eq!(page["items"][0]["id"], third["id"]);
    assert_eq!(page["items"][1]["id"], second["id"]);
    let cursor = page["next_cursor"].as_str().unwrap();
    h.json(
        Method::POST,
        "/v1/projects",
        &owner,
        json!({"request_id":Uuid::new_v4(),"name":"After snapshot"}),
        201,
    )
    .await;
    let page2 = h
        .json(
            Method::GET,
            &format!("/v1/projects?cursor={cursor}&limit=2"),
            &owner,
            Value::Null,
            200,
        )
        .await;
    assert_eq!(page2["items"].as_array().unwrap().len(), 1);
    assert_eq!(page2["items"][0]["id"], a["id"]);
    assert_eq!(page2["next_cursor"], Value::Null);
    // A chat cursor cannot be reused with another project or with its filter removed.
    for title in ["one", "two"] {
        h.json(
            Method::POST,
            "/v1/chats",
            &owner,
            json!({"title":title,"project_id":second["id"]}),
            201,
        )
        .await;
    }
    let second_id = second["id"].as_str().unwrap();
    let third_id = third["id"].as_str().unwrap();
    let chats = h
        .json(
            Method::GET,
            &format!("/v1/chats?project_id={second_id}&limit=1"),
            &owner,
            Value::Null,
            200,
        )
        .await;
    let chat_cursor = chats["next_cursor"].as_str().unwrap();
    h.status(
        Method::GET,
        &format!("/v1/chats?project_id={third_id}&cursor={chat_cursor}"),
        &owner,
        Value::Null,
        400,
    )
    .await;
    h.status(
        Method::GET,
        &format!("/v1/chats?cursor={chat_cursor}"),
        &owner,
        Value::Null,
        400,
    )
    .await;
    h.status(
        Method::GET,
        &format!("/v1/chats?project_id={second_id}"),
        &other,
        Value::Null,
        404,
    )
    .await;
    h.status(
        Method::GET,
        &format!("/v1/projects?cursor={cursor}"),
        &other,
        Value::Null,
        400,
    )
    .await;
    h.status(
        Method::GET,
        "/v1/projects?cursor=malformed",
        &owner,
        Value::Null,
        400,
    )
    .await;
    h.status(
        Method::GET,
        "/v1/projects?limit=101",
        &owner,
        Value::Null,
        400,
    )
    .await;
    h.status(
        Method::GET,
        "/v1/projects?limit=0",
        &owner,
        Value::Null,
        400,
    )
    .await;
    h.status(
        Method::POST,
        "/v1/projects",
        &owner,
        json!({"request_id":Uuid::new_v4(),"name":" \n\t "}),
        400,
    )
    .await;
    h.json(
        Method::POST,
        "/v1/projects",
        &owner,
        json!({"request_id":Uuid::new_v4(),"name":"é".repeat(200)}),
        201,
    )
    .await;
    // Public property: Unicode names over 200 characters always fail, regardless of bytes.
    let mut runner = proptest::test_runner::TestRunner::new(proptest::test_runner::Config {
        cases: 8,
        rng_seed: proptest::test_runner::RngSeed::Fixed(73641986),
        ..Default::default()
    });
    for _ in 0..8 {
        let length = (201usize..400).new_tree(&mut runner).unwrap().current();
        h.status(
            Method::POST,
            "/v1/projects",
            &owner,
            json!({"request_id":Uuid::new_v4(),"name":"界".repeat(length)}),
            400,
        )
        .await;
    }
}
