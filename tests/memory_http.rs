//! Public personal-memory lifecycle contracts against production migrations/PostgreSQL 18.
use axum::{Json, Router, extract::State, routing::post};
use proptest::strategy::{Strategy, ValueTree};
use serde_json::{Value, json};
use smartcindy::{Application, Config, ProviderConfig};
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use testcontainers::{
    GenericImage, ImageExt,
    core::{IntoContainerPort, WaitFor},
    runners::AsyncRunner,
};
use uuid::Uuid;

const DIGEST: &str = "sha256:ad249f9fb9572979643a47694e59b9a8cdcdf6abc6accebf49bb80ad5f20ff9b";

async fn blocked_text_provider(
    State(calls): State<Arc<AtomicUsize>>,
    Json(_body): Json<Value>,
) -> axum::http::StatusCode {
    calls.fetch_add(1, Ordering::SeqCst);
    std::future::pending::<()>().await;
    axum::http::StatusCode::OK
}

#[tokio::test]
async fn authenticated_memory_lifecycle_is_bounded_versioned_and_owner_scoped() {
    let db = GenericImage::new("pgvector/pgvector", &format!("pg18@{DIGEST}"))
        .with_exposed_port(5432.tcp())
        .with_wait_for(WaitFor::message_on_stderr(
            "listening on IPv4 address \"0.0.0.0\", port 5432",
        ))
        .with_env_var("POSTGRES_PASSWORD", "synthetic-only-password")
        .start()
        .await
        .expect("Docker/PostgreSQL required; do not skip");
    let url = format!(
        "postgres://postgres:synthetic-only-password@127.0.0.1:{}/postgres",
        db.get_host_port_ipv4(5432.tcp()).await.unwrap()
    );
    Application::migrate(&url).await.unwrap();
    let pool = sqlx::PgPool::connect(&url).await.unwrap();
    let app = Application::new(pool, Config::new(url)).await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, app.router()).await.unwrap() });
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();

    let token = signup(&client, &base, "memory-owner@example.com").await;
    let other = signup(&client, &base, "other-owner@example.com").await;
    let settings_url = format!("{base}/v1/memory/settings");
    for token in [&token, &other] {
        let r = client
            .get(format!("{base}/v1/memory/jobs?limit=100"))
            .bearer_auth(token)
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 200);
        assert!(
            r.json::<Value>().await.unwrap()["items"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }
    let r = client
        .get(&settings_url)
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    assert_eq!(
        r.json::<Value>().await.unwrap(),
        json!({"enabled":true,"version":0})
    );

    let memory_url = format!("{base}/v1/memories");
    let request_id = Uuid::new_v4();
    let payload = json!({"request_id":request_id,"content":"I prefer tea."});
    let r = client
        .post(&memory_url)
        .bearer_auth(&token)
        .json(&payload)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 201);
    let memory: Value = r.json().await.unwrap();
    assert_eq!(memory["version"], 0);

    let r = client
        .post(&memory_url)
        .bearer_auth(&token)
        .json(&payload)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    assert_eq!(r.json::<Value>().await.unwrap()["id"], memory["id"]);
    let r = client
        .post(&memory_url)
        .bearer_auth(&token)
        .json(&json!({"request_id":request_id,"content":"Different payload"}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 409);

    let r = client
        .patch(format!("{memory_url}/{}", memory["id"].as_str().unwrap()))
        .bearer_auth(&token)
        .json(&json!({"version":0,"content":"I prefer green tea."}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let updated: Value = r.json().await.unwrap();
    assert_eq!(updated["version"], 1);
    assert_eq!(updated["content"], "I prefer green tea.");
    let r = client
        .patch(format!("{memory_url}/{}", memory["id"].as_str().unwrap()))
        .bearer_auth(&token)
        .json(&json!({"version":0,"content":"stale write"}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 409);

    let r = client
        .get(&memory_url)
        .bearer_auth(&other)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    assert_eq!(
        r.json::<Value>().await.unwrap()["items"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
    let r = client
        .patch(format!("{memory_url}/{}", memory["id"].as_str().unwrap()))
        .bearer_auth(&other)
        .json(&json!({"version":1,"content":"cross account"}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 404);

    for content in ["A second memory", "A third memory"] {
        let r = client
            .post(&memory_url)
            .bearer_auth(&token)
            .json(&json!({"request_id":Uuid::new_v4(),"content":content}))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 201);
    }
    let r = client
        .get(format!("{memory_url}?limit=2"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    let page: Value = r.json().await.unwrap();
    assert_eq!(page["items"].as_array().unwrap().len(), 2);
    assert!(page["next_cursor"].is_string());
    let cursor = page["next_cursor"].as_str().unwrap();
    let r = client
        .get(format!("{memory_url}?limit=2&cursor={cursor}"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(
        r.json::<Value>().await.unwrap()["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        client
            .get(format!("{memory_url}?limit=101"))
            .bearer_auth(&token)
            .send()
            .await
            .unwrap()
            .status(),
        400
    );
    assert_eq!(
        client
            .post(&memory_url)
            .bearer_auth(&token)
            .json(&json!({"request_id":Uuid::new_v4(),"content":"   \n  "}))
            .send()
            .await
            .unwrap()
            .status(),
        400
    );
    assert_eq!(
        client
            .patch(&settings_url)
            .bearer_auth(&token)
            .json(&json!({"version":-1,"enabled":false}))
            .send()
            .await
            .unwrap()
            .status(),
        400
    );

    let r = client
        .patch(&settings_url)
        .bearer_auth(&token)
        .json(&json!({"version":4,"enabled":false}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    assert_eq!(
        r.json::<Value>().await.unwrap(),
        json!({"enabled":false,"version":5})
    );
    assert_eq!(
        client
            .post(&memory_url)
            .bearer_auth(&token)
            .json(&json!({"request_id":Uuid::new_v4(),"content":"disabled"}))
            .send()
            .await
            .unwrap()
            .status(),
        409
    );
    let r = client
        .get(&memory_url)
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(
        r.json::<Value>().await.unwrap()["items"]
            .as_array()
            .unwrap()
            .len(),
        3,
        "disabling memory preserves saved memories for later re-enablement"
    );
    let r = client
        .patch(&settings_url)
        .bearer_auth(&token)
        .json(&json!({"version":5,"enabled":true}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.json::<Value>().await.unwrap()["version"], 6);

    let id = memory["id"].as_str().unwrap();
    let r = client
        .delete(format!("{memory_url}/{id}?version=1"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 204);
    let r = client
        .post(&memory_url)
        .bearer_auth(&token)
        .json(&payload)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let tombstone: Value = r.json().await.unwrap();
    assert_eq!(tombstone["id"], memory["id"]);
    assert_eq!(tombstone["content"], "");
    assert_eq!(
        client
            .post(&memory_url)
            .bearer_auth(&token)
            .json(&json!({"request_id":Uuid::new_v4(),"content":"x".repeat(2001)}))
            .send()
            .await
            .unwrap()
            .status(),
        400
    );

    // A fixed-seed public property checks UTF-8 byte caps and negative optimistic versions.
    let mut runner = proptest::test_runner::TestRunner::new(proptest::test_runner::Config {
        cases: 8,
        rng_seed: proptest::test_runner::RngSeed::Fixed(73641983),
        ..Default::default()
    });
    let lengths = (1usize..=8).new_tree(&mut runner).unwrap();
    let mut lengths = lengths;
    for _ in 0..8 {
        let count = lengths.current();
        let oversize = "é".repeat(1001 + count);
        let r = client
            .post(&memory_url)
            .bearer_auth(&token)
            .json(&json!({"request_id":Uuid::new_v4(),"content":oversize}))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 400);
        let r = client
            .patch(format!("{memory_url}/{id}"))
            .bearer_auth(&token)
            .json(&json!({"version":-(count as i64),"content":"invalid version"}))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 400);
        lengths.simplify();
    }
    server.abort();
}

#[tokio::test]
async fn memory_mutation_cancels_running_text_generation() {
    let db = GenericImage::new("pgvector/pgvector", &format!("pg18@{DIGEST}"))
        .with_exposed_port(5432.tcp())
        .with_wait_for(WaitFor::message_on_stderr(
            "listening on IPv4 address \"0.0.0.0\", port 5432",
        ))
        .with_env_var("POSTGRES_PASSWORD", "synthetic-only-password")
        .start()
        .await
        .expect("Docker/PostgreSQL required; do not skip");
    let url = format!(
        "postgres://postgres:synthetic-only-password@127.0.0.1:{}/postgres",
        db.get_host_port_ipv4(5432.tcp()).await.unwrap()
    );
    Application::migrate(&url).await.unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let simulator_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let simulator_origin = format!("http://{}", simulator_listener.local_addr().unwrap());
    let simulator = tokio::spawn({
        let calls = calls.clone();
        async move {
            axum::serve(
                simulator_listener,
                Router::new()
                    .route("/chat/completions", post(blocked_text_provider))
                    .with_state(calls),
            )
            .await
            .unwrap();
        }
    });
    let pool = sqlx::PgPool::connect(&url).await.unwrap();
    let mut config = Config::new(url);
    config.providers = ProviderConfig {
        deepseek_key: Some("synthetic-text-key".into()),
        bfl_key: None,
        local_simulator: Some(simulator_origin),
    };
    let app = Application::new(pool, config).await.unwrap();
    let _user = app
        .provision_user("memory-cancel@example.com", "synthetic-password-123".into())
        .await
        .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server_app = app.clone();
    let server = tokio::spawn(async move {
        axum::serve(listener, server_app.router()).await.unwrap();
    });
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    let response = client
        .post(format!("{base}/v1/sessions"))
        .json(&json!({"email":"memory-cancel@example.com","password":"synthetic-password-123"}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let token = response.json::<Value>().await.unwrap()["token"]
        .as_str()
        .unwrap()
        .to_owned();
    let response = client
        .post(format!("{base}/v1/chats"))
        .bearer_auth(&token)
        .json(&json!({"title":"memory cancellation"}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 201);
    let chat = response.json::<Value>().await.unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let response = client
        .post(format!("{base}/v1/chats/{chat}/turns"))
        .bearer_auth(&token)
        .json(&json!({"request_id":Uuid::new_v4(),"parent_id":null,"parts":[{"type":"text","text":"A running turn"}],"output":"text","replaces_message_id":null}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 202);
    let generation = response.json::<Value>().await.unwrap()["generation_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let worker = app.run_worker();
    tokio::time::timeout(Duration::from_secs(5), async {
        while calls.load(Ordering::SeqCst) == 0 {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    let response = client
        .post(format!("{base}/v1/memories"))
        .bearer_auth(&token)
        .json(&json!({"request_id":Uuid::new_v4(),"content":"A corrected personal fact"}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 201);
    let response = client
        .get(format!("{base}/v1/generations/{generation}"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let state: Value = response.json().await.unwrap();
    assert_eq!(state["status"], "cancelled");
    assert_eq!(state["error"], "memory_changed");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    worker.shutdown().await.unwrap();
    server.abort();
    simulator.abort();
}

async fn signup(client: &reqwest::Client, base: &str, email: &str) -> String {
    let r = client
        .post(format!("{base}/v1/accounts"))
        .json(&json!({
            "email":email,
            "password":"synthetic-password-123",
            "display_name":"Memory User"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 201);
    r.json::<Value>().await.unwrap()["token"]
        .as_str()
        .unwrap()
        .to_owned()
}
