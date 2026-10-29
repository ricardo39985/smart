//! Public cache invalidation/replay journeys; PostgreSQL triggers are real.
use proptest::strategy::{Strategy, ValueTree};
use reqwest::{Client, Method, Response};
use serde_json::{Value, json};
use smartcindy::{Application, Config};
use std::time::Duration;
use testcontainers::{
    ContainerAsync, GenericImage, ImageExt,
    core::{IntoContainerPort, WaitFor},
    runners::AsyncRunner,
};
use uuid::Uuid;

struct Harness {
    _db: ContainerAsync<GenericImage>,
    pool: sqlx::PgPool,
    client: Client,
    base: String,
    server: tokio::task::JoinHandle<()>,
}
impl Drop for Harness {
    fn drop(&mut self) {
        self.server.abort();
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
        .with_env_var("POSTGRES_PASSWORD", "synthetic-realtime-password")
        .start()
        .await
        .expect("Docker/PostgreSQL required; never skip");
        let url = format!(
            "postgres://postgres:synthetic-realtime-password@127.0.0.1:{}/postgres",
            db.get_host_port_ipv4(5432.tcp()).await.unwrap()
        );
        Application::migrate(&url).await.unwrap();
        let pool = sqlx::PgPool::connect(&url).await.unwrap();
        let app = Application::new(pool.clone(), Config::new(url))
            .await
            .unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            axum::serve(listener, app.router()).await.unwrap();
        });
        Self {
            _db: db,
            pool,
            client: Client::builder()
                .connect_timeout(Duration::from_secs(3))
                .build()
                .unwrap(),
            base,
            server,
        }
    }
    async fn signup(&self, email: &str) -> (String, Uuid) {
        let r=self.client.post(format!("{}/v1/accounts",self.base)).json(&json!({"email":email,"display_name":"Private Name","password":"synthetic-password-123"})).send().await.unwrap();
        assert_eq!(r.status(), 201);
        let value: Value = r.json().await.unwrap();
        (
            value["token"].as_str().unwrap().into(),
            serde_json::from_value(value["user_id"].clone()).unwrap(),
        )
    }
    async fn request(
        &self,
        method: Method,
        path: &str,
        token: &str,
        body: Value,
        status: u16,
    ) -> Response {
        let r = self
            .client
            .request(method, format!("{}{}", self.base, path))
            .bearer_auth(token)
            .json(&body)
            .timeout(Duration::from_secs(5))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), status, "{path}");
        r
    }
    async fn stream(&self, token: &str, after: Option<i64>) -> Events {
        let mut request = self
            .client
            .get(format!("{}/v1/events", self.base))
            .bearer_auth(token);
        if let Some(after) = after {
            request = request.header("Last-Event-ID", after);
        }
        let response = tokio::time::timeout(Duration::from_secs(5), request.send())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(response.status(), 200);
        assert_eq!(response.headers()["content-type"], "text/event-stream");
        Events {
            response,
            pending: String::new(),
        }
    }
}
struct Events {
    response: Response,
    pending: String,
}
impl Events {
    async fn next(&mut self) -> (String, i64, Value) {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(end) = self.pending.find("\n\n") {
                    let frame = self.pending[..end].to_owned();
                    self.pending.drain(..end + 2);
                    let mut kind = None;
                    let mut id = None;
                    let mut data = None;
                    for line in frame.lines() {
                        if let Some(v) = line.strip_prefix("event:") {
                            kind = Some(v.trim().to_owned());
                        }
                        if let Some(v) = line.strip_prefix("id:") {
                            id = Some(v.trim().parse::<i64>().unwrap());
                        }
                        if let Some(v) = line.strip_prefix("data:") {
                            data = Some(serde_json::from_str::<Value>(v.trim()).unwrap());
                        }
                    }
                    if let (Some(kind), Some(id), Some(data)) = (kind, id, data) {
                        return (kind, id, data);
                    }
                } else {
                    let bytes = self
                        .response
                        .chunk()
                        .await
                        .unwrap()
                        .expect("expected event before stream closed");
                    self.pending.push_str(std::str::from_utf8(&bytes).unwrap());
                }
            }
        })
        .await
        .expect("event deadline")
    }
    async fn closed(&mut self) {
        tokio::time::timeout(Duration::from_secs(5), async {
            while self.response.chunk().await.unwrap().is_some() {}
        })
        .await
        .expect("revoked stream must close");
    }
}

#[tokio::test]
async fn changes_are_private_coalesced_replayable_and_revoked_with_the_session() {
    let h = Harness::new().await;
    let (owner, owner_id) = h.signup("events-owner@example.com").await;
    let (other, _) = h.signup("events-other@example.com").await;
    h.request(Method::GET, "/v1/events", "", Value::Null, 401)
        .await;
    let mut events = h.stream(&owner, None).await;
    let (kind, initial, body) = events.next().await;
    assert_eq!(kind, "resync");
    assert_eq!(body, json!({"seq":initial,"action":"resync"}));
    drop(events);

    let foreign: Value = h
        .request(
            Method::POST,
            "/v1/chats",
            &other,
            json!({"title":"Foreign secret title"}),
            201,
        )
        .await
        .json()
        .await
        .unwrap();
    let chat: Value = h
        .request(
            Method::POST,
            "/v1/chats",
            &owner,
            json!({"title":"Private travel plans"}),
            201,
        )
        .await
        .json()
        .await
        .unwrap();
    let id = chat["id"].as_str().unwrap();
    h.request(
        Method::PATCH,
        &format!("/v1/chats/{id}"),
        &owner,
        json!({"version":1,"title":"Private renamed title"}),
        200,
    )
    .await;
    h.request(
        Method::DELETE,
        &format!("/v1/chats/{id}?version=2"),
        &owner,
        Value::Null,
        204,
    )
    .await;
    let project: Value = h
        .request(
            Method::POST,
            "/v1/projects",
            &owner,
            json!({"request_id":Uuid::new_v4(),"name":"Private project name"}),
            201,
        )
        .await
        .json()
        .await
        .unwrap();
    let memory: Value = h
        .request(
            Method::POST,
            "/v1/memories",
            &owner,
            json!({"request_id":Uuid::new_v4(),"content":"I prefer private quiet rooms."}),
            201,
        )
        .await
        .json()
        .await
        .unwrap();
    h.request(
        Method::PATCH,
        "/v1/account",
        &owner,
        json!({"display_name":"Private updated name"}),
        200,
    )
    .await;
    let mut replay = h.stream(&owner, Some(initial)).await;
    let (kind, last, body) = replay.next().await;
    assert_eq!(kind, "changes");
    assert!(last > initial);
    assert_eq!(body["seq"], last);
    let changes = body["changes"].as_array().unwrap();
    assert_eq!(changes.iter().filter(|v| v["id"] == chat["id"]).count(), 1);
    assert!(changes.contains(&json!({"resource":"chat","id":chat["id"],"action":"remove"})));
    assert!(
        changes.contains(&json!({"resource":"project","id":project["id"],"action":"invalidate"}))
    );
    assert!(
        changes.contains(&json!({"resource":"memory","id":memory["id"],"action":"invalidate"}))
    );
    assert!(changes.iter().any(|v| v["resource"] == "memory_settings"));
    assert!(changes.iter().any(|v| v["resource"] == "account"));
    assert!(!changes.iter().any(|v| v["id"] == foreign["id"]));
    for change in changes {
        assert_eq!(change.as_object().unwrap().len(), 3);
    }
    assert!(!body.to_string().contains("Private"));
    assert!(!body.to_string().contains("quiet rooms"));
    drop(replay);

    // Resume acknowledges the complete coalesced batch, not just its last resource.
    let new_chat: Value = h
        .request(
            Method::POST,
            "/v1/chats",
            &owner,
            json!({"title":"New"}),
            201,
        )
        .await
        .json()
        .await
        .unwrap();
    let mut resumed = h.stream(&owner, Some(last)).await;
    let (_, next, body) = resumed.next().await;
    assert_eq!(next, last + 1);
    assert_eq!(
        body["changes"],
        json!([{"resource":"chat","id":new_chat["id"],"action":"invalidate"}])
    );
    // A private photo is a leaf mutation, but still invalidates the account view.
    let photo = image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
        1,
        1,
        image::Rgba([0, 0, 0, 255]),
    ));
    let mut encoded = std::io::Cursor::new(Vec::new());
    photo
        .write_to(&mut encoded, image::ImageFormat::Png)
        .unwrap();
    let uploaded = h
        .client
        .put(format!("{}/v1/account/avatar", h.base))
        .bearer_auth(&owner)
        .header("content-type", "image/png")
        .body(encoded.into_inner())
        .send()
        .await
        .unwrap();
    assert_eq!(uploaded.status(), 200);
    let (_, photo_cursor, body) = resumed.next().await;
    assert!(photo_cursor > next);
    assert_eq!(
        body["changes"],
        json!([{"resource":"account","id":owner_id,"action":"invalidate"}])
    );
    h.request(
        Method::DELETE,
        "/v1/account/avatar",
        &owner,
        Value::Null,
        204,
    )
    .await;
    let (_, _, body) = resumed.next().await;
    assert_eq!(
        body["changes"],
        json!([{"resource":"account","id":owner_id,"action":"invalidate"}])
    );
    h.request(
        Method::DELETE,
        "/v1/sessions/current",
        &owner,
        Value::Null,
        204,
    )
    .await;
    resumed.closed().await;
}

#[tokio::test]
async fn retained_history_gaps_resync_and_invalid_cursors_never_replay() {
    let h = Harness::new().await;
    let (token, owner) = h.signup("events-retention@example.com").await;
    let mut stream = h.stream(&token, None).await;
    let (_, old, _) = stream.next().await;
    drop(stream);
    // Persistence-boundary fixture: exercise the production trigger and retention
    // in one owner-locked bulk transaction, then observe replay through public HTTP.
    let mut tx = h.pool.begin().await.unwrap();
    sqlx::query("SELECT id FROM users WHERE id=$1 FOR UPDATE")
        .bind(owner)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("INSERT INTO chats(id,owner_id,title) SELECT gen_random_uuid(),$1,'Synthetic retention fixture' FROM generate_series(1,1005)").bind(owner).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    let retained: i64 =
        sqlx::query_scalar("SELECT count(*) FROM realtime_events WHERE owner_id=$1")
            .bind(owner)
            .fetch_one(&h.pool)
            .await
            .unwrap();
    assert_eq!(retained, 1000);
    let mut replay = h.stream(&token, Some(old)).await;
    let (kind, high, body) = replay.next().await;
    assert_eq!(kind, "resync");
    assert_eq!(body, json!({"seq":high,"action":"resync"}));
    assert_eq!(high, old + 1005);
    drop(replay);
    h.request(
        Method::GET,
        &format!("/v1/events?after={}", high + 1),
        &token,
        Value::Null,
        400,
    )
    .await;
    let r = h
        .client
        .get(format!("{}/v1/events?after=0", h.base))
        .bearer_auth(&token)
        .header("Last-Event-ID", high)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 400);
    h.request(
        Method::GET,
        "/v1/events?token=not-a-credential",
        &token,
        Value::Null,
        400,
    )
    .await;
    let mut runner = proptest::test_runner::TestRunner::new(proptest::test_runner::Config {
        cases: 8,
        rng_seed: proptest::test_runner::RngSeed::Fixed(73641988),
        ..Default::default()
    });
    for _ in 0..8 {
        let cursor = (i64::MIN..0_i64).new_tree(&mut runner).unwrap().current();
        h.request(
            Method::GET,
            &format!("/v1/events?after={cursor}"),
            &token,
            Value::Null,
            400,
        )
        .await;
    }
    // Expire the complete retained window, preserving the durable high watermark.
    sqlx::query("UPDATE realtime_events SET created_at=now()-interval '8 days' WHERE owner_id=$1")
        .bind(owner)
        .execute(&h.pool)
        .await
        .unwrap();
    let mut expired = h.stream(&token, Some(high - 1)).await;
    assert_eq!(expired.next().await.0, "resync");
    drop(expired);

    let mut streams = Vec::new();
    for _ in 0..32 {
        let mut stream = h.stream(&token, None).await;
        stream.next().await;
        streams.push(stream);
    }
    h.request(Method::GET, "/v1/events", &token, Value::Null, 429)
        .await;
    drop(streams);
    // Capacity returns after the peer disconnect reaches the server.
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let r = h
                .client
                .get(format!("{}/v1/events", h.base))
                .bearer_auth(&token)
                .send()
                .await
                .unwrap();
            if r.status() == 200 {
                break;
            }
            assert_eq!(r.status(), 429);
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
}
