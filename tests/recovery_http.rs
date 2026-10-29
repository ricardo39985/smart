//! SQL is used only to inject an expired running lease: the state left by a killed process.
//! Outcomes and retry behavior are asserted through authenticated public HTTP.
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

struct Task(tokio::task::JoinHandle<()>);
impl Drop for Task {
    fn drop(&mut self) {
        self.0.abort();
    }
}
async fn wait(client: &reqwest::Client, base: &str, token: &str, generation: &str) -> Value {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let r = client
                .get(format!("{base}/v1/generations/{generation}"))
                .bearer_auth(token)
                .send()
                .await
                .unwrap();
            assert_eq!(r.status(), 200);
            let value: Value = r.json().await.unwrap();
            if matches!(value["status"].as_str(), Some("failed" | "completed")) {
                return value;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .unwrap()
}
#[tokio::test]
async fn expired_paid_outcome_is_never_resubmitted_but_durable_queued_work_survives_restart() {
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
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let provider=axum::Router::new().route("/chat/completions",axum::routing::post(move ||{let counter=counter.clone();async move {
        counter.fetch_add(1,Ordering::SeqCst);
        ([("content-type","text/event-stream")],"data: {\"choices\":[{\"delta\":{\"content\":\"recovered\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n")
    }}));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let _provider = Task(tokio::spawn(async move {
        axum::serve(listener, provider).await.unwrap();
    }));
    let mut config = Config::new(url.clone());
    config.providers = ProviderConfig {
        deepseek_key: Some("synthetic".into()),
        bfl_key: None,
        local_simulator: Some(origin),
    };
    let app = Application::connect(config.clone()).await.unwrap();
    app.provision_user("recovery@example.test", "synthetic-password".into())
        .await
        .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let routes = app.router();
    let _server = Task(tokio::spawn(async move {
        axum::serve(listener, routes).await.unwrap();
    }));
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(12))
        .build()
        .unwrap();
    let login: Value = client
        .post(format!("{base}/v1/sessions"))
        .json(&json!({"email":"recovery@example.test","password":"synthetic-password"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let token = login["token"].as_str().unwrap();
    let chat: Value = client
        .post(format!("{base}/v1/chats"))
        .bearer_auth(token)
        .json(&json!({"title":"recovery"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let chat = chat["id"].as_str().unwrap();
    let payload = json!({"request_id":Uuid::new_v4(),"parent_id":null,"parts":[{"type":"text","text":"unknown paid outcome"}],"output":"text","replaces_message_id":null});
    let r = client
        .post(format!("{base}/v1/chats/{chat}/turns"))
        .bearer_auth(token)
        .json(&payload)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 202);
    let accepted: Value = r.json().await.unwrap();
    let generation = accepted["generation_id"].as_str().unwrap();
    let pool = sqlx::PgPool::connect(&url).await.unwrap();
    sqlx::query("UPDATE generations SET status='running',lease=$2,lease_until=now()-interval '1 minute',provider_receipt='synthetic-unknown-outcome' WHERE id=$1")
        .bind(Uuid::parse_str(generation).unwrap()).bind(Uuid::new_v4()).execute(&pool).await.unwrap();
    pool.close().await;
    let restarted = Application::connect(config).await.unwrap();
    let workers = restarted.run_worker();
    let failed = wait(&client, &base, token, generation).await;
    assert_eq!(failed["status"], "failed");
    assert_eq!(failed["error"], "interrupted_unknown_provider_outcome");
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let retry = client
        .post(format!("{base}/v1/chats/{chat}/turns"))
        .bearer_auth(token)
        .json(&payload)
        .send()
        .await
        .unwrap();
    assert_eq!(retry.status(), 202);
    assert_eq!(
        retry.json::<Value>().await.unwrap()["generation_id"],
        generation
    );
    workers.shutdown().await.unwrap();
    let mut queued = payload.clone();
    queued["request_id"] = json!(Uuid::new_v4());
    queued["parts"][0]["text"] = json!("safe queued operation");
    let r = client
        .post(format!("{base}/v1/chats/{chat}/turns"))
        .bearer_auth(token)
        .json(&queued)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 202);
    let queued: Value = r.json().await.unwrap();
    let workers = restarted.run_worker();
    let done = wait(
        &client,
        &base,
        token,
        queued["generation_id"].as_str().unwrap(),
    )
    .await;
    assert_eq!(done["status"], "completed");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    workers.shutdown().await.unwrap();
}
