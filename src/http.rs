use crate::{Application, attachments, auth, chat, identity, memory, profile_photo, projects};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Request, State},
    http::{StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub(crate) struct Error(pub StatusCode, pub &'static str);
pub(crate) type Result<T> = std::result::Result<T, Error>;
impl Error {
    pub fn bad() -> Self {
        Self(StatusCode::BAD_REQUEST, "invalid_request")
    }
    pub fn missing() -> Self {
        Self(StatusCode::NOT_FOUND, "not_found")
    }
    pub fn conflict() -> Self {
        Self(StatusCode::CONFLICT, "conflict")
    }
    pub fn unavailable() -> Self {
        Self(StatusCode::SERVICE_UNAVAILABLE, "capability_unavailable")
    }
}
impl From<sqlx::Error> for Error {
    fn from(e: sqlx::Error) -> Self {
        if let sqlx::Error::Database(ref d) = e
            && d.code().as_deref() == Some("23505")
        {
            return Self::conflict();
        }
        Self(StatusCode::INTERNAL_SERVER_ERROR, "internal_error")
    }
}
impl IntoResponse for Error {
    fn into_response(self) -> Response {
        json_response(self.0, json!({"error":{"code":self.1,"message":self.1}}))
    }
}
pub(crate) fn json_response(status: StatusCode, value: Value) -> Response {
    match sonic_rs::to_vec(&value) {
        Ok(bytes) => (
            status,
            [
                (header::CONTENT_TYPE, "application/json"),
                (header::CACHE_CONTROL, "no-store"),
            ],
            bytes,
        )
            .into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}
// Normalize extractor rejections as well as route errors to the public envelope.
async fn envelope(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    if response.status().is_client_error()
        && !response.headers().contains_key(header::CACHE_CONTROL)
    {
        let status = match response.status() {
            StatusCode::UNPROCESSABLE_ENTITY | StatusCode::UNSUPPORTED_MEDIA_TYPE => {
                StatusCode::BAD_REQUEST
            }
            other => other,
        };
        let code = match status {
            StatusCode::PAYLOAD_TOO_LARGE => "payload_too_large",
            StatusCode::NOT_FOUND => "not_found",
            _ => "invalid_request",
        };
        response = Error(status, code).into_response();
    }
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-store"),
    );
    response
}
pub(crate) fn router(app: Application) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .route(
            "/v1/sessions",
            post(auth::login).layer(DefaultBodyLimit::max(8192)),
        )
        .route("/v1/sessions/current", delete(auth::logout))
        .route(
            "/v1/accounts",
            post(identity::signup).layer(DefaultBodyLimit::max(8192)),
        )
        .route(
            "/v1/account",
            get(identity::account)
                .patch(identity::profile)
                .layer(DefaultBodyLimit::max(8192)),
        )
        .route(
            "/v1/account/avatar",
            get(profile_photo::get)
                .put(profile_photo::put)
                .delete(profile_photo::remove)
                .layer(DefaultBodyLimit::max(2 * 1024 * 1024)),
        )
        .route(
            "/v1/account/email-change",
            post(identity::email_change_disabled).layer(DefaultBodyLimit::max(8192)),
        )
        .route(
            "/v1/account/password-change",
            post(identity::password_change_disabled).layer(DefaultBodyLimit::max(8192)),
        )
        .route("/v1/auth/methods", get(identity::methods))
        .route(
            "/v1/auth/exchange",
            post(identity::exchange).layer(DefaultBodyLimit::max(8192)),
        )
        .route(
            "/v1/auth/{provider}/start",
            post(identity::start).layer(DefaultBodyLimit::max(8192)),
        )
        .route(
            "/v1/auth/{provider}/callback",
            get(identity::google_callback)
                .post(identity::apple_callback)
                .layer(DefaultBodyLimit::max(8192)),
        )
        .route_layer(DefaultBodyLimit::max(16 * 1024))
        .route("/v1/capabilities", get(capabilities))
        .route("/v1/events", get(crate::realtime::events))
        .route(
            "/v1/devices/{id}",
            axum::routing::put(crate::push::register).delete(crate::push::remove),
        )
        .route("/v1/memory/jobs", get(memory::jobs))
        .route(
            "/v1/memory/settings",
            get(memory::settings).patch(memory::settings_update),
        )
        .route("/v1/memories", get(memory::list).post(memory::create))
        .route(
            "/v1/memories/{id}",
            axum::routing::patch(memory::update).delete(memory::remove),
        )
        .route("/v1/projects", get(projects::list).post(projects::create))
        .route(
            "/v1/projects/{id}",
            get(projects::get)
                .patch(projects::update)
                .delete(projects::remove),
        )
        .route("/v1/chats/{id}/project", post(projects::move_chat))
        .route("/v1/chats", get(chat::list).post(chat::create))
        .route(
            "/v1/chats/{id}",
            get(chat::get).patch(chat::update).delete(chat::remove),
        )
        .route("/v1/chats/{id}/generation", get(chat::chat_generation))
        .route("/v1/chats/{id}/messages", get(chat::messages))
        .route("/v1/chats/{id}/branches", get(chat::branches))
        .route(
            "/v1/chats/{id}/turns",
            post(chat::turn).layer(DefaultBodyLimit::max(256 * 1024)),
        )
        .route(
            "/v1/chats/{id}/messages/{message}/regenerations",
            post(chat::regenerate),
        )
        .route("/v1/generations/{id}", get(chat::generation))
        .route("/v1/generations/{id}/cancel", post(chat::cancel))
        .route("/v1/generations/{id}/events", get(chat::events))
        .route(
            "/v1/attachments",
            post(attachments::upload).layer(DefaultBodyLimit::max(8 * 1024 * 1024)),
        )
        .route(
            "/v1/attachments/{id}",
            get(attachments::download).delete(attachments::remove),
        )
        .fallback(|| async { Error::missing() })
        .layer(DefaultBodyLimit::max(16 * 1024))
        .layer(middleware::from_fn(envelope))
        .with_state(app)
}
async fn healthz(State(app): State<Application>) -> Response {
    let ready = tokio::time::timeout(std::time::Duration::from_secs(1), async {
        sqlx::query("SELECT 1").execute(&app.0.pool).await
    })
    .await
    .is_ok_and(|result| result.is_ok());
    if ready {
        json_response(StatusCode::OK, json!({"status":"ok"}))
    } else {
        json_response(
            StatusCode::SERVICE_UNAVAILABLE,
            json!({"status":"unavailable"}),
        )
    }
}
async fn capabilities(State(app): State<Application>, _auth: auth::Auth) -> Json<Value> {
    Json(
        json!({"push":crate::push::available(&app.0.config.push),"push_android":app.0.config.push.fcm.is_some(),"push_ios":app.0.config.push.apns.is_some(),"text":app.0.config.providers.deepseek_key.is_some(),"image_input":app.0.config.providers.deepseek_key.is_some() || app.0.config.providers.bfl_key.is_some(),"image_output":app.0.config.providers.bfl_key.is_some(),"image_editing":app.0.config.providers.bfl_key.is_some()}),
    )
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Cursor {
    pub v: u8,
    pub kind: String,
    pub owner: uuid::Uuid,
    pub chat: Option<uuid::Uuid>,
    pub archived: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<uuid::Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub search: Option<String>,
    pub high: i64,
    pub before: i64,
    pub head: Option<uuid::Uuid>,
    pub next: Option<uuid::Uuid>,
}
pub(crate) fn encode(c: &Cursor) -> String {
    URL_SAFE_NO_PAD.encode(serde_json::to_vec(c).expect("serializable cursor"))
}
pub(crate) fn decode(
    s: &str,
    kind: &str,
    owner: uuid::Uuid,
    chat: Option<uuid::Uuid>,
) -> Result<Cursor> {
    if s.len() > 1024 {
        return Err(Error::bad());
    }
    let c: Cursor = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(s).map_err(|_| Error::bad())?)
        .map_err(|_| Error::bad())?;
    if c.v != 1
        || c.kind != kind
        || c.owner != owner
        || c.chat != chat
        || c.high < 0
        || c.before < 0
        || c.before > c.high
    {
        return Err(Error::bad());
    }
    Ok(c)
}
pub(crate) fn limit(n: Option<i64>, default: i64) -> Result<i64> {
    let n = n.unwrap_or(default);
    if !(1..=100).contains(&n) {
        return Err(Error::bad());
    }
    Ok(n)
}
