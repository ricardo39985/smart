//! Session-scoped native devices and content-minimal durable completion notifications.
use crate::{
    Application,
    auth::Auth,
    http::{Error, Result},
};
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use futures_util::StreamExt;
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{Postgres, Row, Transaction};
use std::time::{Duration, Instant};
use tokio::sync::watch;
use uuid::Uuid;

#[derive(Clone, Default)]
pub struct PushConfig {
    pub fcm: Option<FcmConfig>,
    pub apns: Option<ApnsConfig>,
    /// Explicit loopback protocol simulator; never set by the production CLI.
    pub local_simulator: Option<String>,
}
#[derive(Clone)]
pub struct FcmConfig {
    pub project_id: String,
    pub client_email: String,
    pub private_key: String,
}
#[derive(Clone)]
pub struct ApnsConfig {
    pub team_id: String,
    pub key_id: String,
    pub bundle_id: String,
    pub private_key: String,
    pub sandbox: bool,
}

pub(crate) fn available(config: &PushConfig) -> bool {
    config.fcm.is_some() || config.apns.is_some()
}
pub(crate) fn validate_config(config: &PushConfig) -> anyhow::Result<()> {
    if let Some(url) = &config.local_simulator {
        let url = reqwest::Url::parse(url)?;
        anyhow::ensure!(
            url.scheme() == "http"
                && matches!(url.host_str(), Some("127.0.0.1" | "[::1]"))
                && url.username().is_empty()
                && url.password().is_none()
                && url.query().is_none()
                && url.fragment().is_none()
                && url.path() == "/",
            "push simulator must be loopback origin"
        );
    }
    if let Some(c) = &config.fcm {
        anyhow::ensure!(
            (6..=64).contains(&c.project_id.len())
                && c.project_id
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
                && crate::auth::email(&c.client_email).is_some(),
            "invalid FCM configuration"
        );
        EncodingKey::from_rsa_pem(c.private_key.as_bytes())?;
    }
    if let Some(c) = &config.apns {
        anyhow::ensure!(
            [&c.team_id, &c.key_id]
                .iter()
                .all(|s| s.len() == 10 && s.bytes().all(|b| b.is_ascii_alphanumeric()))
                && !c.bundle_id.is_empty()
                && c.bundle_id.len() <= 255
                && c.bundle_id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-')),
            "invalid APNs configuration"
        );
        EncodingKey::from_ec_pem(c.private_key.as_bytes())?;
    }
    Ok(())
}

fn yes() -> bool {
    true
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Registration {
    platform: String,
    token: String,
    #[serde(default = "yes")]
    enabled: bool,
    #[serde(default)]
    previews: bool,
}
pub(crate) async fn register(
    State(app): State<Application>,
    auth: Auth,
    Path(id): Path<Uuid>,
    Json(input): Json<Registration>,
) -> Result<Json<Value>> {
    if !matches!(input.platform.as_str(), "android" | "ios")
        || input.token.is_empty()
        || input.token.len() > 4096
        || input
            .token
            .chars()
            .any(|c| c.is_whitespace() || c.is_control())
    {
        return Err(Error::bad());
    }
    // APNs path tokens must be hex; opaque Android tokens never enter a URL.
    if input.platform == "ios" && !input.token.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(Error::bad());
    }
    let token = if input.platform == "ios" {
        input.token.to_ascii_lowercase()
    } else {
        input.token
    };
    let token_hash = Sha256::digest(token.as_bytes()).to_vec();
    // Expiry is irreversible. Reclaim one stale token before taking any owner/device
    // locks, so concurrent cross-account recovery cannot invert their lock order.
    sqlx::query("DELETE FROM devices d USING sessions s WHERE d.token_hash=$1 AND s.token_hash=d.session_hash AND s.user_id=d.owner_id AND s.expires_at<=now()")
        .bind(&token_hash).execute(&app.0.pool).await?;
    let mut tx = app.0.pool.begin().await?;
    sqlx::query("SELECT id FROM users WHERE id=$1 FOR UPDATE")
        .bind(auth.user)
        .execute(&mut *tx)
        .await?;
    sqlx::query("SELECT token_hash FROM sessions WHERE user_id=$1 AND token_hash=$2 AND expires_at>now() FOR KEY SHARE")
        .bind(auth.user).bind(&auth.hash).fetch_optional(&mut *tx).await?.ok_or(Error(StatusCode::UNAUTHORIZED,"unauthorized"))?;
    let existing =
        sqlx::query("SELECT id,platform,enabled,previews,updated_at,session_hash,token_hash FROM devices WHERE owner_id=$1 AND id=$2 FOR UPDATE")
            .bind(auth.user)
            .bind(id)
            .fetch_optional(&mut *tx)
            .await?;
    if let Some(row) = &existing {
        if row.get::<Vec<u8>, _>("session_hash") == auth.hash
            && row.get::<Vec<u8>, _>("token_hash") == token_hash
            && row.get::<String, _>("platform") == input.platform
            && row.get::<bool, _>("enabled") == input.enabled
            && row.get::<bool, _>("previews") == input.previews
        {
            let response = registration_json(row);
            tx.commit().await?;
            return Ok(Json(response));
        }
    } else {
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM devices WHERE owner_id=$1")
            .bind(auth.user)
            .fetch_one(&mut *tx)
            .await?;
        if count >= 32 {
            return Err(Error(StatusCode::TOO_MANY_REQUESTS, "device_limit"));
        }
    }
    // Foreign owner/device token reuse conflicts rather than silently transferring a recipient.
    let row=sqlx::query("INSERT INTO devices(owner_id,id,session_hash,platform,token,token_hash,enabled,previews) VALUES($1,$2,$3,$4,$5,$6,$7,$8) ON CONFLICT(owner_id,id) DO UPDATE SET session_hash=EXCLUDED.session_hash,platform=EXCLUDED.platform,token=EXCLUDED.token,token_hash=EXCLUDED.token_hash,enabled=EXCLUDED.enabled,previews=EXCLUDED.previews,version=devices.version+1,updated_at=now() RETURNING id,platform,enabled,previews,updated_at")
        .bind(auth.user).bind(id).bind(auth.hash).bind(input.platform).bind(token).bind(token_hash).bind(input.enabled).bind(input.previews).fetch_one(&mut *tx).await?;
    sqlx::query("UPDATE push_outbox SET status='cancelled',payload='{}',lease=NULL,lease_until=NULL,updated_at=now() WHERE owner_id=$1 AND device_id=$2 AND status IN ('queued','sending')")
        .bind(auth.user).bind(id).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(registration_json(&row)))
}
fn registration_json(row: &sqlx::postgres::PgRow) -> Value {
    json!({"id":row.get::<Uuid,_>("id"),"platform":row.get::<String,_>("platform"),"enabled":row.get::<bool,_>("enabled"),"previews":row.get::<bool,_>("previews"),"updated_at":row.get::<chrono::DateTime<chrono::Utc>,_>("updated_at")})
}
pub(crate) async fn remove(
    State(app): State<Application>,
    auth: Auth,
    Path(id): Path<Uuid>,
) -> Result<StatusCode> {
    let deleted = sqlx::query("DELETE FROM devices WHERE owner_id=$1 AND id=$2")
        .bind(auth.user)
        .bind(id)
        .execute(&app.0.pool)
        .await?
        .rows_affected();
    if deleted == 0 {
        return Err(Error::missing());
    }
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize, Deserialize)]
struct Notification {
    chat_id: Uuid,
    generation_id: Uuid,
    title: String,
    body: String,
    deep_link: String,
    attachment_id: Option<String>,
    copy_reply: bool,
}
pub(crate) async fn enqueue(
    tx: &mut Transaction<'_, Postgres>,
    app: &Application,
    owner: Uuid,
    chat: Uuid,
    generation: Uuid,
) -> sqlx::Result<()> {
    let config = &app.0.config.push;
    if !available(config) {
        return Ok(());
    }
    // Keep each recipient alive through its outbox FK insert. Device-before-outbox
    // ordering matches registration, settlement, and logout/device-delete cascades.
    let rows=sqlx::query("SELECT d.id,d.version,d.previews,g.output,CASE WHEN d.previews THEN left(COALESCE(m.parts->0->>'text',''),160) ELSE '' END AS preview,CASE WHEN d.previews AND g.output='image' THEN m.parts->0->>'attachment_id' ELSE NULL END AS attachment FROM devices d JOIN sessions s ON s.token_hash=d.session_hash AND s.user_id=d.owner_id JOIN generations g ON g.id=$2 AND g.owner_id=d.owner_id JOIN messages m ON m.id=g.result_message_id WHERE d.owner_id=$1 AND d.enabled AND s.expires_at>now() AND ((d.platform='android' AND $3) OR (d.platform='ios' AND $4)) ORDER BY d.id LIMIT 32 FOR KEY SHARE OF d")
        .bind(owner).bind(generation).bind(config.fcm.is_some()).bind(config.apns.is_some()).fetch_all(&mut **tx).await?;
    let mut values = Vec::new();
    for row in rows {
        let output: String = row.get("output");
        let preview: String = row.get("preview");
        let body = if preview.is_empty() {
            if output == "image" {
                "Your image is ready.".into()
            } else {
                "Open your conversation.".into()
            }
        } else {
            preview
        };
        let payload = json!(Notification {
            chat_id: chat,
            generation_id: generation,
            title: "Cindy replied".into(),
            body,
            deep_link: format!(
                "{}://chat/{chat}?generation={generation}",
                app.0
                    .config
                    .identity
                    .app_scheme
                    .as_deref()
                    .unwrap_or("smartcindy")
            ),
            attachment_id: row.get("attachment"),
            copy_reply: output == "text"
        });
        values.push(json!({"id":Uuid::new_v4(),"device_id":row.get::<Uuid,_>("id"),"device_version":row.get::<i64,_>("version"),"payload":payload}));
    }
    if values.is_empty() {
        return Ok(());
    }
    // Coalesce queued notifications for the same chat; an in-flight provider request cannot be recalled.
    sqlx::query("UPDATE push_outbox SET status='cancelled',payload='{}',updated_at=now() WHERE owner_id=$1 AND chat_id=$2 AND status='queued'").bind(owner).bind(chat).execute(&mut **tx).await?;
    sqlx::query("INSERT INTO push_outbox(id,owner_id,device_id,device_version,chat_id,generation_id,payload,status,error) SELECT p.id,$2,p.device_id,p.device_version,$3,$4,CASE WHEN (SELECT count(*) FROM push_outbox WHERE owner_id=$2 AND status IN ('queued','sending'))<=992 THEN p.payload ELSE '{}'::jsonb END,CASE WHEN (SELECT count(*) FROM push_outbox WHERE owner_id=$2 AND status IN ('queued','sending'))<=992 THEN 'queued' ELSE 'failed' END,CASE WHEN (SELECT count(*) FROM push_outbox WHERE owner_id=$2 AND status IN ('queued','sending'))<=992 THEN NULL ELSE 'queue_full' END FROM jsonb_to_recordset($1) AS p(id uuid,device_id uuid,device_version bigint,payload jsonb) ON CONFLICT(generation_id,owner_id,device_id) DO NOTHING")
        .bind(json!(values)).bind(owner).bind(chat).bind(generation).execute(&mut **tx).await?;
    Ok(())
}

struct Job {
    id: Uuid,
    owner: Uuid,
    device: Uuid,
    version: i64,
    lease: Uuid,
    token: String,
    platform: String,
    payload: Notification,
    attempts: i32,
}
enum Delivery {
    Sent,
    Invalid,
    Retry(u64),
    Failed,
}
#[derive(Default)]
struct Tokens {
    fcm: Option<(String, Instant)>,
    apns: Option<(String, Instant)>,
}

pub(crate) async fn run(
    app: Application,
    mut shutdown: watch::Receiver<bool>,
) -> anyhow::Result<()> {
    let mut tokens = Tokens::default();
    loop {
        if *shutdown.borrow() {
            return Ok(());
        }
        if available(&app.0.config.push) {
            maintenance(&app).await?;
            if let Some(job) = claim(&app).await? {
                let delivery = tokio::select! {
                    _=shutdown.changed()=>return Ok(()),
                    result=deliver(&app,&job,&mut tokens)=>result.unwrap_or(Delivery::Retry(60)),
                };
                settle(&app, &job, delivery).await?;
                continue;
            }
        }
        tokio::select! {_=shutdown.changed()=>return Ok(()),_=tokio::time::sleep(Duration::from_secs(1))=>{}}
    }
}
async fn maintenance(app: &Application) -> sqlx::Result<()> {
    // Lock only outbox candidates before changing them. A stale maintenance snapshot
    // must neither wait behind another worker nor overwrite its newly claimed lease.
    sqlx::query("UPDATE push_outbox SET status=CASE WHEN attempts<3 THEN 'queued' ELSE 'failed' END,payload=CASE WHEN attempts<3 THEN payload ELSE '{}'::jsonb END,error='interrupted',lease=NULL,lease_until=NULL,next_attempt=now()+interval '60 seconds',updated_at=now() WHERE id IN (SELECT p.id FROM push_outbox p WHERE p.status='sending' AND p.lease_until<now() LIMIT 32 FOR UPDATE OF p SKIP LOCKED)").execute(&app.0.pool).await?;
    sqlx::query("UPDATE push_outbox p SET status='cancelled',payload='{}',updated_at=now() WHERE p.id IN (SELECT p.id FROM push_outbox p JOIN devices d ON d.owner_id=p.owner_id AND d.id=p.device_id JOIN sessions s ON s.token_hash=d.session_hash JOIN chats c ON c.id=p.chat_id WHERE p.status='queued' AND (NOT d.enabled OR s.expires_at<=now() OR c.deleted_at IS NOT NULL OR p.device_version<>d.version OR p.created_at<now()-interval '1 hour') LIMIT 32 FOR UPDATE OF p SKIP LOCKED)").execute(&app.0.pool).await?;
    sqlx::query("DELETE FROM push_outbox WHERE id IN (SELECT id FROM push_outbox WHERE status IN ('sent','failed','cancelled') AND updated_at<now()-interval '7 days' LIMIT 100)").execute(&app.0.pool).await?;
    Ok(())
}
async fn claim(app: &Application) -> anyhow::Result<Option<Job>> {
    let mut tx = app.0.pool.begin().await?;
    let row=sqlx::query("SELECT p.*,d.token,d.platform FROM push_outbox p JOIN devices d ON d.owner_id=p.owner_id AND d.id=p.device_id JOIN sessions s ON s.user_id=d.owner_id AND s.token_hash=d.session_hash JOIN chats c ON c.id=p.chat_id AND c.owner_id=p.owner_id WHERE p.status='queued' AND p.attempts<3 AND p.next_attempt<=now() AND p.created_at>now()-interval '1 hour' AND d.enabled AND d.version=p.device_version AND s.expires_at>now() AND c.deleted_at IS NULL AND ((d.platform='android' AND $1) OR (d.platform='ios' AND $2)) ORDER BY p.created_at,p.id FOR UPDATE OF p SKIP LOCKED LIMIT 1")
        .bind(app.0.config.push.fcm.is_some()).bind(app.0.config.push.apns.is_some()).fetch_optional(&mut *tx).await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let id: Uuid = row.get("id");
    let lease = Uuid::new_v4();
    sqlx::query("UPDATE push_outbox SET status='sending',attempts=attempts+1,lease=$2,lease_until=now()+interval '30 seconds',updated_at=now() WHERE id=$1").bind(id).bind(lease).execute(&mut *tx).await?;
    let job = Job {
        id,
        owner: row.get("owner_id"),
        device: row.get("device_id"),
        version: row.get("device_version"),
        lease,
        token: row.get("token"),
        platform: row.get("platform"),
        payload: serde_json::from_value(row.get("payload"))?,
        attempts: row.get::<i32, _>("attempts") + 1,
    };
    tx.commit().await?;
    Ok(Some(job))
}
async fn settle(app: &Application, job: &Job, delivery: Delivery) -> sqlx::Result<()> {
    let mut tx = app.0.pool.begin().await?;
    // Device-before-outbox is also registration/cascade order; stale leases have no effects.
    if sqlx::query("SELECT id FROM devices WHERE owner_id=$1 AND id=$2 FOR UPDATE")
        .bind(job.owner)
        .bind(job.device)
        .fetch_optional(&mut *tx)
        .await?
        .is_none()
    {
        return Ok(());
    }
    if sqlx::query("SELECT id FROM push_outbox WHERE id=$1 AND lease=$2 AND status='sending' AND lease_until>now() FOR UPDATE")
        .bind(job.id).bind(job.lease).fetch_optional(&mut *tx).await?.is_none() {
        return Ok(());
    }
    let (status, error, delay) = match delivery {
        Delivery::Sent => ("sent", None, 0),
        Delivery::Invalid => {
            sqlx::query("UPDATE devices SET enabled=false,version=version+1,updated_at=now() WHERE owner_id=$1 AND id=$2 AND version=$3").bind(job.owner).bind(job.device).bind(job.version).execute(&mut *tx).await?;
            ("failed", Some("invalid_token"), 0)
        }
        Delivery::Retry(wait) if job.attempts < 3 && wait <= 3600 => (
            "queued",
            Some("transient_provider_error"),
            wait.max(60u64 << (job.attempts - 1)) + (job.id.as_bytes()[0] as u64 % 15),
        ),
        _ => ("failed", Some("provider_rejected"), 0),
    };
    sqlx::query("UPDATE push_outbox SET status=$3,error=$4,next_attempt=now()+make_interval(secs=>$5),lease=NULL,lease_until=NULL,updated_at=now(),payload=CASE WHEN $3='queued' THEN payload ELSE '{}'::jsonb END WHERE id=$1 AND lease=$2 AND status='sending' AND lease_until>now()")
        .bind(job.id).bind(job.lease).bind(status).bind(error).bind(delay as f64).execute(&mut *tx).await?;
    tx.commit().await
}

async fn small_json(response: reqwest::Response) -> anyhow::Result<Value> {
    anyhow::ensure!(
        response.content_length().is_none_or(|n| n <= 32768),
        "push response limit"
    );
    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        anyhow::ensure!(bytes.len() + chunk.len() <= 32768, "push response limit");
        bytes.extend_from_slice(&chunk);
    }
    if bytes.is_empty() {
        Ok(Value::Null)
    } else {
        Ok(serde_json::from_slice(&bytes)?)
    }
}
fn payload_data(n: &Notification) -> Value {
    let mut actions = vec![
        json!({"id":"OPEN_CHAT","title":"Open"}),
        json!({"id":"REPLY","title":"Reply","input":"text"}),
    ];
    if n.copy_reply {
        actions.push(json!({"id":"COPY_REPLY","title":"Copy reply"}));
    }
    let mut data = json!({"type":"conversation_updated","chat_id":n.chat_id.to_string(),"generation_id":n.generation_id.to_string(),"deep_link":n.deep_link,"actions":json!(actions).to_string()});
    if let Some(id) = &n.attachment_id {
        data["attachment_id"] = json!(id);
    }
    data
}
async fn deliver(app: &Application, job: &Job, tokens: &mut Tokens) -> anyhow::Result<Delivery> {
    let valid:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM push_outbox p JOIN devices d ON d.owner_id=p.owner_id AND d.id=p.device_id JOIN sessions s ON s.user_id=d.owner_id AND s.token_hash=d.session_hash JOIN chats c ON c.id=p.chat_id WHERE p.id=$1 AND p.lease=$2 AND p.status='sending' AND p.lease_until>now() AND d.enabled AND d.version=p.device_version AND s.expires_at>now() AND c.deleted_at IS NULL)")
        .bind(job.id).bind(job.lease).fetch_one(&app.0.pool).await?;
    if !valid {
        return Ok(Delivery::Failed);
    }
    if job.platform == "android" {
        fcm(app, job, tokens).await
    } else {
        apns(app, job, tokens).await
    }
}
async fn fcm(app: &Application, job: &Job, tokens: &mut Tokens) -> anyhow::Result<Delivery> {
    let c = app
        .0
        .config
        .push
        .fcm
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("FCM disabled"))?;
    if tokens
        .fcm
        .as_ref()
        .is_none_or(|(_, expires)| *expires <= Instant::now())
    {
        let now = chrono::Utc::now().timestamp();
        let assertion = jsonwebtoken::encode(
            &Header::new(Algorithm::RS256),
            &json!({"iss":c.client_email,"scope":"https://www.googleapis.com/auth/firebase.messaging","aud":"https://oauth2.googleapis.com/token","iat":now,"exp":now+3600}),
            &EncodingKey::from_rsa_pem(c.private_key.as_bytes())?,
        )?;
        let endpoint = app
            .0
            .config
            .push
            .local_simulator
            .as_ref()
            .map(|b| format!("{}/oauth/token", b.trim_end_matches('/')))
            .unwrap_or_else(|| "https://oauth2.googleapis.com/token".into());
        let response=app.0.client.post(endpoint).timeout(Duration::from_secs(10)).header("content-type","application/x-www-form-urlencoded")
            .body(format!("grant_type=urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Ajwt-bearer&assertion={assertion}")).send().await?;
        if !response.status().is_success() {
            return Ok(Delivery::Retry(60));
        }
        let body = small_json(response).await?;
        let token = body["access_token"]
            .as_str()
            .filter(|s| !s.is_empty() && s.len() <= 8192)
            .ok_or_else(|| anyhow::anyhow!("invalid FCM authorization"))?;
        let seconds = body["expires_in"]
            .as_u64()
            .filter(|n| *n > 60 && *n <= 3600)
            .ok_or_else(|| anyhow::anyhow!("invalid FCM expiry"))?;
        tokens.fcm = Some((
            token.into(),
            Instant::now() + Duration::from_secs(seconds - 60),
        ));
    }
    let n = &job.payload;
    let mut message = json!({"notification":{"title":n.title,"body":n.body},"data":payload_data(n),"android":{"priority":"high","ttl":"3600s","collapse_key":n.chat_id.to_string(),"notification":{"channel_id":"conversation_updates","click_action":"OPEN_CHAT","tag":n.chat_id.to_string()}}});
    anyhow::ensure!(
        serde_json::to_vec(&message)?.len() <= 4096,
        "push payload limit"
    );
    message["token"] = json!(job.token);
    let base = app
        .0
        .config
        .push
        .local_simulator
        .as_deref()
        .unwrap_or("https://fcm.googleapis.com");
    let response = app
        .0
        .client
        .post(format!(
            "{}/v1/projects/{}/messages:send",
            base.trim_end_matches('/'),
            c.project_id
        ))
        .timeout(Duration::from_secs(10))
        .bearer_auth(&tokens.fcm.as_ref().expect("initialized token").0)
        .json(&json!({"message":message}))
        .send()
        .await?;
    let status = response.status();
    let retry = retry_after(&response);
    let body = small_json(response).await?;
    if status.is_success() {
        return Ok(Delivery::Sent);
    }
    if body
        .pointer("/error/details")
        .and_then(Value::as_array)
        .is_some_and(|details| details.iter().any(|v| v["errorCode"] == "UNREGISTERED"))
    {
        return Ok(Delivery::Invalid);
    }
    if status == StatusCode::UNAUTHORIZED {
        tokens.fcm = None;
        return Ok(Delivery::Retry(60));
    }
    if status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error() {
        return Ok(Delivery::Retry(retry));
    }
    Ok(Delivery::Failed)
}
async fn apns(app: &Application, job: &Job, tokens: &mut Tokens) -> anyhow::Result<Delivery> {
    let c = app
        .0
        .config
        .push
        .apns
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("APNs disabled"))?;
    if tokens
        .apns
        .as_ref()
        .is_none_or(|(_, expires)| *expires <= Instant::now())
    {
        let mut header = Header::new(Algorithm::ES256);
        header.kid = Some(c.key_id.clone());
        let token = jsonwebtoken::encode(
            &header,
            &json!({"iss":c.team_id,"iat":chrono::Utc::now().timestamp()}),
            &EncodingKey::from_ec_pem(c.private_key.as_bytes())?,
        )?;
        tokens.apns = Some((token, Instant::now() + Duration::from_secs(3000)));
    }
    let n = &job.payload;
    let mut payload = payload_data(n);
    payload["aps"] = json!({"alert":{"title":n.title,"subtitle":"Your conversation is ready","body":n.body},"sound":"default","category":if n.copy_reply {"CHAT_REPLY"}else{"CHAT_IMAGE"},"thread-id":n.chat_id.to_string(),"interruption-level":"active","content-available":1});
    if n.attachment_id.is_some() {
        payload["aps"]["mutable-content"] = json!(1);
    }
    anyhow::ensure!(
        serde_json::to_vec(&payload)?.len() <= 4096,
        "push payload limit"
    );
    let base = app
        .0
        .config
        .push
        .local_simulator
        .as_deref()
        .unwrap_or(if c.sandbox {
            "https://api.sandbox.push.apple.com"
        } else {
            "https://api.push.apple.com"
        });
    let mut request = app
        .0
        .client
        .post(format!(
            "{}/3/device/{}",
            base.trim_end_matches('/'),
            job.token
        ))
        .timeout(Duration::from_secs(10))
        .bearer_auth(&tokens.apns.as_ref().expect("initialized token").0)
        .header("apns-topic", &c.bundle_id)
        .header("apns-push-type", "alert")
        .header("apns-priority", "10")
        .header("apns-id", job.id.to_string())
        .header("apns-collapse-id", n.chat_id.to_string())
        .header(
            "apns-expiration",
            (chrono::Utc::now().timestamp() + 3600).to_string(),
        )
        .json(&payload);
    if app.0.config.push.local_simulator.is_none() {
        request = request.version(reqwest::Version::HTTP_2);
    }
    let response = request.send().await?;
    let status = response.status();
    let retry = retry_after(&response);
    let body = small_json(response).await?;
    if status.is_success() {
        return Ok(Delivery::Sent);
    }
    if matches!(
        body["reason"].as_str(),
        Some("Unregistered" | "BadDeviceToken" | "DeviceTokenNotForTopic")
    ) {
        return Ok(Delivery::Invalid);
    }
    if body["reason"] == "ExpiredProviderToken" {
        tokens.apns = None;
        return Ok(Delivery::Retry(60));
    }
    if status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error() {
        return Ok(Delivery::Retry(retry));
    }
    Ok(Delivery::Failed)
}
fn retry_after(response: &reqwest::Response) -> u64 {
    response
        .headers()
        .get("retry-after")
        .and_then(|h| h.to_str().ok())
        .and_then(|s| {
            s.parse().ok().or_else(|| {
                chrono::DateTime::parse_from_rfc2822(s)
                    .ok()
                    .map(|at| (at.timestamp() - chrono::Utc::now().timestamp()).max(0) as u64)
            })
        })
        .unwrap_or(60)
}
