use serde_json::json;
use smartcindy::{Application, Config};
use std::time::Duration;
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

#[tokio::test]
async fn readiness_reports_database_health_without_authentication() {
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
    let pool = sqlx::PgPool::connect(&url).await.unwrap();
    let app = Application::new(pool.clone(), Config::new(url))
        .await
        .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = Task(tokio::spawn(async move {
        axum::serve(listener, app.router()).await.unwrap();
    }));
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
        .unwrap();

    let response = client.get(format!("{base}/healthz")).send().await.unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    assert_eq!(response.headers()["cache-control"], "no-store");
    assert_eq!(
        response.json::<serde_json::Value>().await.unwrap(),
        json!({"status":"ok"})
    );

    let response = client.get(format!("{base}/v1/chats")).send().await.unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::UNAUTHORIZED);

    pool.close().await;
    let response = client.get(format!("{base}/healthz")).send().await.unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(response.headers()["cache-control"], "no-store");
    assert_eq!(
        response.json::<serde_json::Value>().await.unwrap(),
        json!({"status":"unavailable"})
    );
    drop(server);
}
