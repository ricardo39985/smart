use crate::{
    Application,
    auth::Auth,
    http::{Error, Result, limit},
};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{FromRow, Postgres, Row, Transaction};
use uuid::Uuid;

const MAX_CONTENT_BYTES: usize = 2000;
const MAX_ACTIVE: i64 = 1000;

#[derive(Serialize, FromRow)]
struct Memory {
    id: Uuid,
    content: String,
    version: i64,
    source_chat_id: Option<Uuid>,
    source_message_id: Option<Uuid>,
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
    #[serde(skip)]
    seq: i64,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Cursor {
    v: u8,
    kind: String,
    owner: Uuid,
    high: i64,
    before: i64,
}

fn encode_cursor(cursor: &Cursor) -> String {
    URL_SAFE_NO_PAD.encode(serde_json::to_vec(cursor).expect("serializable memory cursor"))
}

fn decode_cursor(s: &str, kind: &str, owner: Uuid) -> Result<Cursor> {
    if s.len() > 1024 {
        return Err(Error::bad());
    }
    let cursor: Cursor =
        serde_json::from_slice(&URL_SAFE_NO_PAD.decode(s).map_err(|_| Error::bad())?)
            .map_err(|_| Error::bad())?;
    if cursor.v != 1
        || cursor.kind != kind
        || cursor.owner != owner
        || cursor.high < 0
        || cursor.before < 0
        || cursor.before > cursor.high
    {
        return Err(Error::bad());
    }
    Ok(cursor)
}

pub(crate) async fn current_settings(
    app: &Application,
    owner: Uuid,
) -> anyhow::Result<(bool, i64)> {
    let row = sqlx::query("SELECT memory_enabled, memory_version FROM users WHERE id=$1")
        .bind(owner)
        .fetch_optional(&app.0.pool)
        .await?
        .ok_or_else(|| anyhow::anyhow!("memory settings owner not found"))?;
    Ok((row.get("memory_enabled"), row.get("memory_version")))
}

pub(crate) async fn settings(State(app): State<Application>, auth: Auth) -> Result<Json<Value>> {
    let (enabled, version) = current_settings(&app, auth.user)
        .await
        .map_err(|_| Error::unavailable())?;
    Ok(Json(json!({"enabled":enabled,"version":version})))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SettingsUpdate {
    version: i64,
    enabled: bool,
}

pub(crate) async fn settings_update(
    State(app): State<Application>,
    auth: Auth,
    Json(input): Json<SettingsUpdate>,
) -> Result<Json<Value>> {
    if input.version < 0 {
        return Err(Error::bad());
    }
    let mut tx = app.0.pool.begin().await?;
    let row = sqlx::query("SELECT memory_version FROM users WHERE id=$1 FOR UPDATE")
        .bind(auth.user)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(Error::missing)?;
    let old: i64 = row.get("memory_version");
    if old != input.version {
        return Err(Error::conflict());
    }
    sqlx::query("UPDATE users SET memory_enabled=$2,memory_version=memory_version+1,memory_after_seq=COALESCE((SELECT max(m.seq) FROM messages m JOIN chats c ON c.id=m.chat_id WHERE c.owner_id=users.id),memory_after_seq) WHERE id=$1")
        .bind(auth.user)
        .bind(input.enabled)
        .execute(&mut *tx)
        .await?;
    if !input.enabled {
        cancel_owner_jobs(&mut tx, auth.user).await?;
    }
    cancel_owner_text_generations(&mut tx, auth.user).await?;
    let version = old + 1;
    tx.commit().await?;
    Ok(Json(json!({"enabled":input.enabled,"version":version})))
}

async fn cancel_owner_jobs(tx: &mut Transaction<'_, Postgres>, owner: Uuid) -> Result<()> {
    sqlx::query("UPDATE memory_jobs SET status='cancelled',lease=NULL,lease_until=NULL WHERE owner_id=$1 AND status IN ('queued','running')")
        .bind(owner)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

pub(crate) async fn cancel_owner_text_generations(
    tx: &mut Transaction<'_, Postgres>,
    owner: Uuid,
) -> sqlx::Result<()> {
    let generations: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM generations WHERE owner_id=$1 AND output='text' AND status='running' FOR UPDATE",
    )
    .bind(owner)
    .fetch_all(&mut **tx)
    .await?;
    for id in generations {
        crate::chat::terminal(tx, id, "cancelled", Some("memory_changed")).await?;
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Listing {
    limit: Option<i64>,
    cursor: Option<String>,
}

pub(crate) async fn list(
    State(app): State<Application>,
    auth: Auth,
    Query(query): Query<Listing>,
) -> Result<Json<Value>> {
    let page_size = limit(query.limit, 30)?;
    let (high, before) = if let Some(encoded) = query.cursor {
        let cursor = decode_cursor(&encoded, "memories", auth.user)?;
        (cursor.high, cursor.before)
    } else {
        let mut tx = app.0.pool.begin().await?;
        sqlx::query("SELECT id FROM users WHERE id=$1 FOR UPDATE")
            .bind(auth.user)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(Error::missing)?;
        let high: i64 = sqlx::query_scalar(
            "SELECT COALESCE(max(seq),0) FROM memories WHERE owner_id=$1 AND deleted_at IS NULL",
        )
        .bind(auth.user)
        .fetch_one(&mut *tx)
        .await?;
        tx.commit().await?;
        (high, high)
    };
    let mut rows = sqlx::query_as::<_, Memory>("SELECT id,content,version,source_chat_id,source_message_id,created_at,updated_at,seq FROM memories WHERE owner_id=$1 AND deleted_at IS NULL AND seq<=$2 AND seq<=$3 ORDER BY seq DESC LIMIT $4")
        .bind(auth.user)
        .bind(high)
        .bind(before)
        .bind(page_size + 1)
        .fetch_all(&app.0.pool)
        .await?;
    let next_cursor = if rows.len() as i64 > page_size {
        rows.pop();
        rows.last().map(|row| {
            encode_cursor(&Cursor {
                v: 1,
                kind: "memories".to_owned(),
                owner: auth.user,
                high,
                before: row.seq - 1,
            })
        })
    } else {
        None
    };
    Ok(Json(json!({"items":rows,"next_cursor":next_cursor})))
}

#[derive(Serialize, FromRow)]
struct MemoryJob {
    id: Uuid,
    status: String,
    error: Option<String>,
    usage: Option<Value>,
    created_at: chrono::DateTime<chrono::Utc>,
    chat_id: Uuid,
    #[serde(skip)]
    seq: i64,
}

pub(crate) async fn jobs(
    State(app): State<Application>,
    auth: Auth,
    Query(query): Query<Listing>,
) -> Result<Json<Value>> {
    let page_size = limit(query.limit, 30)?;
    let (high, before) = if let Some(encoded) = query.cursor {
        let cursor = decode_cursor(&encoded, "memory_jobs", auth.user)?;
        (cursor.high, cursor.before)
    } else {
        let mut tx = app.0.pool.begin().await?;
        sqlx::query("SELECT id FROM users WHERE id=$1 FOR UPDATE")
            .bind(auth.user)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(Error::missing)?;
        let high: i64 =
            sqlx::query_scalar("SELECT COALESCE(max(seq),0) FROM memory_jobs WHERE owner_id=$1")
                .bind(auth.user)
                .fetch_one(&mut *tx)
                .await?;
        tx.commit().await?;
        (high, high)
    };
    let mut rows = sqlx::query_as::<_, MemoryJob>("SELECT id,status,error,usage,created_at,chat_id,seq FROM memory_jobs WHERE owner_id=$1 AND seq<=$2 AND seq<=$3 ORDER BY seq DESC LIMIT $4")
        .bind(auth.user)
        .bind(high)
        .bind(before)
        .bind(page_size + 1)
        .fetch_all(&app.0.pool)
        .await?;
    let next_cursor = if rows.len() as i64 > page_size {
        rows.pop();
        rows.last().map(|row| {
            encode_cursor(&Cursor {
                v: 1,
                kind: "memory_jobs".to_owned(),
                owner: auth.user,
                high,
                before: row.seq - 1,
            })
        })
    } else {
        None
    };
    Ok(Json(json!({"items":rows,"next_cursor":next_cursor})))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Create {
    request_id: Uuid,
    content: String,
}

fn content_valid(content: &str) -> bool {
    !content.trim().is_empty() && content.len() <= MAX_CONTENT_BYTES
}

pub(crate) async fn create(
    State(app): State<Application>,
    auth: Auth,
    Json(input): Json<Create>,
) -> Result<(StatusCode, Json<Value>)> {
    if !content_valid(&input.content) {
        return Err(Error::bad());
    }
    let fingerprint = Sha256::digest(input.content.as_bytes()).to_vec();
    let mut tx = app.0.pool.begin().await?;
    let user = sqlx::query("SELECT memory_enabled FROM users WHERE id=$1 FOR UPDATE")
        .bind(auth.user)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(Error::missing)?;

    if let Some(existing) = sqlx::query(
        "SELECT fingerprint,memory_id FROM memory_requests WHERE owner_id=$1 AND request_id=$2",
    )
    .bind(auth.user)
    .bind(input.request_id)
    .fetch_optional(&mut *tx)
    .await?
    {
        let old_fingerprint: Vec<u8> = existing.get("fingerprint");
        if old_fingerprint != fingerprint {
            return Err(Error::conflict());
        }
        let id: Uuid = existing.get("memory_id");
        let row = sqlx::query_as::<_, Memory>("SELECT id,content,version,source_chat_id,source_message_id,created_at,updated_at,seq FROM memories WHERE id=$1 AND owner_id=$2")
            .bind(id)
            .bind(auth.user)
            .fetch_one(&mut *tx)
            .await?;
        tx.commit().await?;
        return Ok((StatusCode::OK, Json(json!(row))));
    }
    if !user.get::<bool, _>("memory_enabled") {
        return Err(Error::conflict());
    }
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM memories WHERE owner_id=$1 AND deleted_at IS NULL",
    )
    .bind(auth.user)
    .fetch_one(&mut *tx)
    .await?;
    if count >= MAX_ACTIVE {
        return Err(Error::conflict());
    }
    let id = Uuid::new_v4();
    let row = sqlx::query_as::<_, Memory>("INSERT INTO memories(id,owner_id,content) VALUES($1,$2,$3) RETURNING id,content,version,source_chat_id,source_message_id,created_at,updated_at,seq")
        .bind(id)
        .bind(auth.user)
        .bind(&input.content)
        .fetch_one(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO memory_requests(owner_id,request_id,fingerprint,memory_id) VALUES($1,$2,$3,$4)")
        .bind(auth.user)
        .bind(input.request_id)
        .bind(fingerprint)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE users SET memory_version=memory_version+1,memory_after_seq=COALESCE((SELECT max(m.seq) FROM messages m JOIN chats c ON c.id=m.chat_id WHERE c.owner_id=users.id),memory_after_seq) WHERE id=$1")
        .bind(auth.user)
        .execute(&mut *tx)
        .await?;
    cancel_owner_text_generations(&mut tx, auth.user).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(json!(row))))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Update {
    version: i64,
    content: String,
}

pub(crate) async fn update(
    State(app): State<Application>,
    auth: Auth,
    Path(id): Path<Uuid>,
    Json(input): Json<Update>,
) -> Result<Json<Value>> {
    if input.version < 0 || !content_valid(&input.content) {
        return Err(Error::bad());
    }
    let mut tx = app.0.pool.begin().await?;
    sqlx::query("SELECT id FROM users WHERE id=$1 FOR UPDATE")
        .bind(auth.user)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(Error::missing)?;
    let old: i64 = sqlx::query_scalar("SELECT version FROM memories WHERE id=$1 AND owner_id=$2 AND deleted_at IS NULL FOR UPDATE")
        .bind(id)
        .bind(auth.user)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(Error::missing)?;
    if old != input.version {
        return Err(Error::conflict());
    }
    let row = sqlx::query_as::<_, Memory>("UPDATE memories SET content=$3,embedding=NULL,embedding_revision=NULL,version=version+1,updated_at=now() WHERE id=$1 AND owner_id=$2 RETURNING id,content,version,source_chat_id,source_message_id,created_at,updated_at,seq")
        .bind(id)
        .bind(auth.user)
        .bind(input.content)
        .fetch_one(&mut *tx)
        .await?;
    sqlx::query("UPDATE users SET memory_version=memory_version+1,memory_after_seq=COALESCE((SELECT max(m.seq) FROM messages m JOIN chats c ON c.id=m.chat_id WHERE c.owner_id=users.id),memory_after_seq) WHERE id=$1")
        .bind(auth.user)
        .execute(&mut *tx)
        .await?;
    cancel_owner_text_generations(&mut tx, auth.user).await?;
    tx.commit().await?;
    Ok(Json(json!(row)))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Version {
    version: i64,
}

pub(crate) async fn remove(
    State(app): State<Application>,
    auth: Auth,
    Path(id): Path<Uuid>,
    Query(query): Query<Version>,
) -> Result<StatusCode> {
    if query.version < 0 {
        return Err(Error::bad());
    }
    let mut tx = app.0.pool.begin().await?;
    sqlx::query("SELECT id FROM users WHERE id=$1 FOR UPDATE")
        .bind(auth.user)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(Error::missing)?;
    let old: i64 = sqlx::query_scalar("SELECT version FROM memories WHERE id=$1 AND owner_id=$2 AND deleted_at IS NULL FOR UPDATE")
        .bind(id)
        .bind(auth.user)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(Error::missing)?;
    if old != query.version {
        return Err(Error::conflict());
    }
    sqlx::query("UPDATE memories SET content='',embedding=NULL,embedding_revision=NULL,deleted_at=now(),version=version+1,updated_at=now() WHERE id=$1 AND owner_id=$2")
        .bind(id)
        .bind(auth.user)
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE users SET memory_version=memory_version+1,memory_after_seq=COALESCE((SELECT max(m.seq) FROM messages m JOIN chats c ON c.id=m.chat_id WHERE c.owner_id=users.id),memory_after_seq) WHERE id=$1")
        .bind(auth.user)
        .execute(&mut *tx)
        .await?;
    cancel_owner_text_generations(&mut tx, auth.user).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn invalidate_chat(
    tx: &mut Transaction<'_, Postgres>,
    owner: Uuid,
    chat: Uuid,
) -> sqlx::Result<()> {
    sqlx::query("SELECT id FROM users WHERE id=$1 FOR UPDATE")
        .bind(owner)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or(sqlx::Error::RowNotFound)?;
    sqlx::query("UPDATE memories SET content='',embedding=NULL,embedding_revision=NULL,deleted_at=now(),version=version+1,updated_at=now() WHERE owner_id=$1 AND source_chat_id=$2 AND deleted_at IS NULL")
        .bind(owner)
        .bind(chat)
        .execute(&mut **tx)
        .await?;
    sqlx::query("DELETE FROM context_chunks WHERE owner_id=$1 AND chat_id=$2")
        .bind(owner)
        .bind(chat)
        .execute(&mut **tx)
        .await?;
    sqlx::query("DELETE FROM summaries WHERE owner_id=$1 AND chat_id=$2")
        .bind(owner)
        .bind(chat)
        .execute(&mut **tx)
        .await?;
    sqlx::query("UPDATE memory_jobs SET status='cancelled',lease=NULL,lease_until=NULL WHERE owner_id=$1 AND chat_id=$2 AND status IN ('queued','running')")
        .bind(owner)
        .bind(chat)
        .execute(&mut **tx)
        .await?;
    cancel_owner_text_generations(tx, owner).await?;
    sqlx::query("UPDATE users SET memory_version=memory_version+1,memory_after_seq=COALESCE((SELECT max(m.seq) FROM messages m JOIN chats c ON c.id=m.chat_id WHERE c.owner_id=users.id),memory_after_seq) WHERE id=$1")
        .bind(owner)
        .execute(&mut **tx)
        .await?;
    Ok(())
}
