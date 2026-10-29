use crate::{
    Application,
    auth::Auth,
    http::{self, Cursor, Error, Result},
};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{
        IntoResponse, Response,
        sse::{Event, KeepAlive, Sse},
    },
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{FromRow, Row};
use uuid::Uuid;

#[derive(Serialize, FromRow)]
struct Chat {
    id: Uuid,
    title: String,
    archived: bool,
    version: i64,
    head_id: Option<Uuid>,
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
    #[serde(skip)]
    seq: i64,
}
pub(crate) async fn get(
    State(app): State<Application>,
    auth: Auth,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    let chat = sqlx::query_as::<_, Chat>(
        "SELECT * FROM chats WHERE id=$1 AND owner_id=$2 AND deleted_at IS NULL",
    )
    .bind(id)
    .bind(auth.user)
    .fetch_optional(&app.0.pool)
    .await?
    .ok_or_else(Error::missing)?;
    Ok(Json(json!(chat)))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Create {
    title: String,
}
fn title_valid(s: &str) -> bool {
    s.chars().count() <= 200
}
pub(crate) async fn create(
    State(app): State<Application>,
    auth: Auth,
    Json(input): Json<Create>,
) -> Result<(StatusCode, Json<Value>)> {
    if !title_valid(&input.title) {
        return Err(Error::bad());
    }
    let mut tx = app.0.pool.begin().await?;
    sqlx::query("SELECT id FROM users WHERE id=$1 FOR UPDATE")
        .bind(auth.user)
        .execute(&mut *tx)
        .await?;
    let chat = sqlx::query_as::<_, Chat>(
        "INSERT INTO chats(id,owner_id,title) VALUES($1,$2,$3) RETURNING *",
    )
    .bind(Uuid::new_v4())
    .bind(auth.user)
    .bind(input.title)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(json!(chat))))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Listing {
    limit: Option<i64>,
    cursor: Option<String>,
    archived: Option<bool>,
}
pub(crate) async fn list(
    State(app): State<Application>,
    auth: Auth,
    Query(q): Query<Listing>,
) -> Result<Json<Value>> {
    let limit = http::limit(q.limit, 30)?;
    let archived = q.archived.unwrap_or(false);
    let (high, before) = if let Some(s) = q.cursor {
        let c = http::decode(&s, "chats", auth.user, None)?;
        if c.archived != Some(archived) {
            return Err(Error::bad());
        }
        (c.high, c.before)
    } else {
        let mut tx = app.0.pool.begin().await?;
        sqlx::query("SELECT id FROM users WHERE id=$1 FOR UPDATE")
            .bind(auth.user)
            .execute(&mut *tx)
            .await?;
        let high: i64 = sqlx::query_scalar("SELECT COALESCE(max(seq),0) FROM chats WHERE owner_id=$1 AND archived=$2 AND deleted_at IS NULL").bind(auth.user).bind(archived).fetch_one(&mut *tx).await?;
        tx.commit().await?;
        (high, high)
    };
    let mut rows = sqlx::query_as::<_,Chat>("SELECT * FROM chats WHERE owner_id=$1 AND archived=$2 AND deleted_at IS NULL AND seq<=$3 AND seq<=$4 ORDER BY seq DESC LIMIT $5")
        .bind(auth.user).bind(archived).bind(high).bind(before).bind(limit+1).fetch_all(&app.0.pool).await?;
    let next = if rows.len() as i64 > limit {
        rows.pop();
        rows.last().map(|r| {
            http::encode(&Cursor {
                v: 1,
                kind: "chats".into(),
                owner: auth.user,
                chat: None,
                archived: Some(archived),
                high,
                before: r.seq - 1,
                head: None,
                next: None,
            })
        })
    } else {
        None
    };
    Ok(Json(json!({"items":rows,"next_cursor":next})))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Update {
    version: i64,
    title: Option<String>,
    archived: Option<bool>,
}
pub(crate) async fn update(
    State(app): State<Application>,
    auth: Auth,
    Path(id): Path<Uuid>,
    Json(input): Json<Update>,
) -> Result<Json<Value>> {
    if input.title.as_deref().is_some_and(|s| !title_valid(s))
        || (input.title.is_none() && input.archived.is_none())
    {
        return Err(Error::bad());
    }
    let mut tx = app.0.pool.begin().await?;
    let old: i64 = sqlx::query_scalar(
        "SELECT version FROM chats WHERE id=$1 AND owner_id=$2 AND deleted_at IS NULL FOR UPDATE",
    )
    .bind(id)
    .bind(auth.user)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(Error::missing)?;
    if old != input.version {
        return Err(Error::conflict());
    }
    let row = sqlx::query_as::<_,Chat>("UPDATE chats SET title=COALESCE($3,title),archived=COALESCE($4,archived),version=version+1,updated_at=now() WHERE id=$1 AND owner_id=$2 RETURNING *")
        .bind(id).bind(auth.user).bind(input.title).bind(input.archived).fetch_one(&mut *tx).await?;
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
    Query(q): Query<Version>,
) -> Result<StatusCode> {
    let mut tx = app.0.pool.begin().await?;
    sqlx::query("SELECT id FROM users WHERE id=$1 FOR UPDATE")
        .bind(auth.user)
        .execute(&mut *tx)
        .await?;
    let version: i64 = sqlx::query_scalar(
        "SELECT version FROM chats WHERE id=$1 AND owner_id=$2 AND deleted_at IS NULL FOR UPDATE",
    )
    .bind(id)
    .bind(auth.user)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(Error::missing)?;
    if version != q.version {
        return Err(Error::conflict());
    }
    let active: Option<Uuid> = sqlx::query_scalar("SELECT id FROM generations WHERE chat_id=$1 AND owner_id=$2 AND status IN ('queued','running') FOR UPDATE").bind(id).bind(auth.user).fetch_optional(&mut *tx).await?;
    if let Some(g) = active {
        terminal(&mut tx, g, "cancelled", None).await?;
    }
    let affected: Vec<Uuid> = sqlx::query_scalar("SELECT attachment_id FROM message_attachments ma JOIN messages m ON m.id=ma.message_id WHERE m.chat_id=$1").bind(id).fetch_all(&mut *tx).await?;
    sqlx::query("DELETE FROM message_attachments ma USING messages m WHERE ma.message_id=m.id AND m.chat_id=$1").bind(id).execute(&mut *tx).await?;
    sqlx::query("DELETE FROM message_attachments ma USING messages m JOIN chats c ON c.id=m.chat_id WHERE ma.message_id=m.id AND c.owner_id=$1 AND c.deleted_at IS NOT NULL AND ma.attachment_id=ANY($2)").bind(auth.user).bind(&affected).execute(&mut *tx).await?;
    let reclaimed: i64 = sqlx::query_scalar::<_, i64>("DELETE FROM attachments a WHERE a.owner_id=$1 AND a.id=ANY($2) AND NOT EXISTS(SELECT 1 FROM message_attachments ma JOIN messages m ON m.id=ma.message_id JOIN chats c ON c.id=m.chat_id WHERE ma.attachment_id=a.id AND c.deleted_at IS NULL) RETURNING octet_length(a.bytes)::bigint").bind(auth.user).bind(&affected).fetch_all(&mut *tx).await?.into_iter().sum();
    if reclaimed > 0 {
        sqlx::query("UPDATE users SET image_bytes=image_bytes-$2 WHERE id=$1")
            .bind(auth.user)
            .bind(reclaimed)
            .execute(&mut *tx)
            .await?;
    }
    sqlx::query("UPDATE chats SET deleted_at=now(),version=version+1,updated_at=now() WHERE id=$1 AND owner_id=$2").bind(id).bind(auth.user).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Part {
    Text { text: String },
    Image { attachment_id: Uuid },
}
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Output {
    Text,
    Image,
}
impl Output {
    pub fn name(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Image => "image",
        }
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Turn {
    request_id: Uuid,
    parent_id: Option<Uuid>,
    parts: Vec<Part>,
    output: Output,
    replaces_message_id: Option<Uuid>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Regenerate {
    request_id: Uuid,
    output: Output,
}
fn valid_parts(parts: &[Part]) -> Result<()> {
    let text: usize = parts
        .iter()
        .map(|p| match p {
            Part::Text { text } => text.len(),
            _ => 0,
        })
        .sum();
    let images = parts
        .iter()
        .filter(|p| matches!(p, Part::Image { .. }))
        .count();
    if parts.is_empty()
        || parts.len() > 16
        || text > 32768
        || images > 4
        || (text == 0 && images == 0)
    {
        return Err(Error::bad());
    }
    Ok(())
}
fn accepted(row: &sqlx::postgres::PgRow) -> Value {
    json!({"generation_id":row.get::<Uuid,_>("id"),"user_message_id":row.get::<Uuid,_>("user_message_id"),"status":row.get::<String,_>("status")})
}
pub(crate) async fn turn(
    State(app): State<Application>,
    auth: Auth,
    Path(chat): Path<Uuid>,
    Json(input): Json<Turn>,
) -> Result<(StatusCode, Json<Value>)> {
    valid_parts(&input.parts)?;
    let fingerprint = Sha256::digest(
        serde_json::to_vec(&json!(["turn", chat, &input])).map_err(|_| Error::bad())?,
    )
    .to_vec();
    accept(
        &app,
        auth.user,
        chat,
        input.request_id,
        input.output,
        fingerprint,
        Some(input),
        None,
    )
    .await
}
pub(crate) async fn regenerate(
    State(app): State<Application>,
    auth: Auth,
    Path((chat, message)): Path<(Uuid, Uuid)>,
    Json(input): Json<Regenerate>,
) -> Result<(StatusCode, Json<Value>)> {
    let fingerprint = Sha256::digest(
        serde_json::to_vec(&json!(["regenerate", chat, message, &input]))
            .map_err(|_| Error::bad())?,
    )
    .to_vec();
    accept(
        &app,
        auth.user,
        chat,
        input.request_id,
        input.output,
        fingerprint,
        None,
        Some(message),
    )
    .await
}
#[allow(clippy::too_many_arguments)]
async fn accept(
    app: &Application,
    owner: Uuid,
    chat: Uuid,
    request: Uuid,
    output: Output,
    fingerprint: Vec<u8>,
    input: Option<Turn>,
    regeneration: Option<Uuid>,
) -> Result<(StatusCode, Json<Value>)> {
    let mut tx = app.0.pool.begin().await?;
    // Serialize global admission, not execution. Every admission uses this order.
    sqlx::query("SELECT pg_advisory_xact_lock(73641981)")
        .execute(&mut *tx)
        .await?;
    sqlx::query("SELECT id FROM users WHERE id=$1 FOR UPDATE")
        .bind(owner)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "SELECT id FROM chats WHERE id=$1 AND owner_id=$2 AND deleted_at IS NULL FOR UPDATE",
    )
    .bind(chat)
    .bind(owner)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(Error::missing)?;
    if let Some(old) = sqlx::query("SELECT * FROM generations WHERE owner_id=$1 AND request_id=$2")
        .bind(owner)
        .bind(request)
        .fetch_optional(&mut *tx)
        .await?
    {
        if old.get::<Vec<u8>, _>("fingerprint") != fingerprint {
            return Err(Error::conflict());
        }
        return Ok((StatusCode::ACCEPTED, Json(accepted(&old))));
    }
    let available = match output {
        Output::Text => app.0.config.providers.deepseek_key.is_some(),
        Output::Image => app.0.config.providers.bfl_key.is_some(),
    };
    if !available {
        return Err(Error::unavailable());
    }
    let counts = sqlx::query("SELECT count(*)::bigint AS total,count(*) FILTER(WHERE owner_id=$1)::bigint AS own,count(*) FILTER(WHERE chat_id=$2)::bigint AS chat FROM generations WHERE status IN ('queued','running')").bind(owner).bind(chat).fetch_one(&mut *tx).await?;
    if counts.get::<i64, _>("chat") > 0 {
        return Err(Error::conflict());
    }
    if counts.get::<i64, _>("total") >= 128 || counts.get::<i64, _>("own") >= 8 {
        return Err(Error(StatusCode::TOO_MANY_REQUESTS, "queue_full"));
    }
    let (message, depth) = if let Some(input) = input {
        if output == Output::Image
            && !input
                .parts
                .iter()
                .any(|p| matches!(p,Part::Text{text} if !text.trim().is_empty()))
        {
            return Err(Error::bad());
        }
        let depth = if let Some(parent) = input.parent_id {
            sqlx::query_scalar::<_, i32>(
                "SELECT depth+1 FROM messages WHERE id=$1 AND chat_id=$2 AND role='assistant'",
            )
            .bind(parent)
            .bind(chat)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(Error::missing)?
        } else {
            0
        };
        if let Some(original) = input.replaces_message_id {
            let parent: Option<Uuid> = sqlx::query_scalar(
                "SELECT parent_id FROM messages WHERE id=$1 AND chat_id=$2 AND role='user'",
            )
            .bind(original)
            .bind(chat)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(Error::missing)?;
            if parent != input.parent_id {
                return Err(Error::conflict());
            }
        }
        if depth >= 128 {
            return Err(Error(StatusCode::BAD_REQUEST, "context_limit"));
        }
        let message = Uuid::new_v4();
        sqlx::query("INSERT INTO messages(id,chat_id,parent_id,role,parts,depth,replaces_message_id,ancestors) VALUES($1,$2,$3,'user',$4,$5,$6,COALESCE((SELECT ancestors || id FROM messages WHERE id=$3 AND chat_id=$2),'{}'::uuid[]))")
            .bind(message).bind(chat).bind(input.parent_id).bind(json!(input.parts)).bind(depth).bind(input.replaces_message_id).execute(&mut *tx).await?;
        for part in &input.parts {
            if let Part::Image { attachment_id } = part {
                let found = sqlx::query("UPDATE attachments a SET ever_attached=true WHERE a.id=$1 AND a.owner_id=$2 AND (NOT a.ever_attached OR EXISTS(SELECT 1 FROM message_attachments ma JOIN messages m ON m.id=ma.message_id JOIN chats c ON c.id=m.chat_id WHERE ma.attachment_id=a.id AND c.owner_id=$2 AND c.deleted_at IS NULL)) RETURNING id").bind(attachment_id).bind(owner).fetch_optional(&mut *tx).await?;
                if found.is_none() {
                    return Err(Error::missing());
                }
                sqlx::query("INSERT INTO message_attachments(message_id,attachment_id) VALUES($1,$2) ON CONFLICT DO NOTHING").bind(message).bind(attachment_id).execute(&mut *tx).await?;
            }
        }
        (message, depth)
    } else {
        let id = regeneration.ok_or_else(Error::bad)?;
        let row = sqlx::query(
            "SELECT depth,parts FROM messages WHERE id=$1 AND chat_id=$2 AND role='user'",
        )
        .bind(id)
        .bind(chat)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(Error::missing)?;
        let parts: Vec<Part> =
            serde_json::from_value(row.get("parts")).map_err(|_| Error::bad())?;
        valid_parts(&parts)?;
        if output == Output::Image
            && !parts
                .iter()
                .any(|p| matches!(p,Part::Text{text} if !text.trim().is_empty()))
        {
            return Err(Error::bad());
        }
        (id, row.get::<i32, _>("depth"))
    };
    // Validate complete context budget before accepting paid work; never truncate silently.
    let budget = sqlx::query("WITH RECURSIVE path AS (SELECT id,parent_id,parts,role,1 AS n FROM messages WHERE id=$1 AND chat_id=$2 UNION ALL SELECT m.id,m.parent_id,m.parts,m.role,p.n+1 FROM messages m JOIN path p ON m.id=p.parent_id WHERE m.chat_id=$2 AND p.n<129) SELECT count(*)::bigint AS n,COALESCE(sum(octet_length(parts::text)),0)::bigint AS bytes,count(*) FILTER(WHERE role='user' AND parts @> '[{\"type\":\"image\"}]'::jsonb)::bigint AS photos FROM path")
        .bind(message).bind(chat).fetch_one(&mut *tx).await?;
    if budget.get::<i64, _>("n") > 128 || budget.get::<i64, _>("bytes") > 65536 || depth >= 128 {
        return Err(Error(StatusCode::BAD_REQUEST, "context_limit"));
    }
    let image_bytes: i64 = sqlx::query_scalar("WITH RECURSIVE path AS (SELECT id,parent_id FROM messages WHERE id=$1 AND chat_id=$2 UNION ALL SELECT m.id,m.parent_id FROM messages m JOIN path p ON m.id=p.parent_id WHERE m.chat_id=$2) SELECT COALESCE(sum(octet_length(a.bytes)),0)::bigint FROM path p JOIN message_attachments ma ON ma.message_id=p.id JOIN attachments a ON a.id=ma.attachment_id AND a.owner_id=$3 WHERE NOT $4 OR p.id=$1")
        .bind(message).bind(chat).bind(owner).bind(output==Output::Image).fetch_one(&mut *tx).await?;
    if image_bytes > 32 * 1024 * 1024 {
        return Err(Error(StatusCode::BAD_REQUEST, "context_limit"));
    }
    let id = Uuid::new_v4();
    let row = sqlx::query("INSERT INTO generations(id,owner_id,chat_id,user_message_id,request_id,fingerprint,output,status) VALUES($1,$2,$3,$4,$5,$6,$7,'queued') RETURNING *")
        .bind(id).bind(owner).bind(chat).bind(message).bind(request).bind(fingerprint).bind(output.name()).fetch_one(&mut *tx).await?;
    // User messages can have multiple regenerations, so generation_id is assistant-only.
    sqlx::query("UPDATE chats SET head_id=$3,version=version+1,updated_at=now() WHERE id=$1 AND owner_id=$2").bind(chat).bind(owner).bind(message).execute(&mut *tx).await?;
    event(
        &mut tx,
        id,
        "accepted",
        json!({"generation_id":id,"user_message_id":message}),
    )
    .await?;
    tx.commit().await?;
    Ok((StatusCode::ACCEPTED, Json(accepted(&row))))
}

pub(crate) async fn event(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    generation: Uuid,
    kind: &str,
    data: Value,
) -> std::result::Result<(), sqlx::Error> {
    let seq: i64 = sqlx::query_scalar("UPDATE generations SET next_seq=next_seq+1,updated_at=now() WHERE id=$1 RETURNING next_seq-1").bind(generation).fetch_one(&mut **tx).await?;
    sqlx::query("INSERT INTO generation_events(generation_id,seq,kind,data) VALUES($1,$2,$3,$4)")
        .bind(generation)
        .bind(seq)
        .bind(kind)
        .bind(data)
        .execute(&mut **tx)
        .await?;
    Ok(())
}
pub(crate) async fn terminal(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    generation: Uuid,
    status: &str,
    error: Option<&str>,
) -> std::result::Result<(), sqlx::Error> {
    sqlx::query("UPDATE generations SET status=$2,error=$3,lease=NULL,lease_until=NULL,updated_at=now() WHERE id=$1").bind(generation).bind(status).bind(error).execute(&mut **tx).await?;
    event(
        tx,
        generation,
        status,
        json!({"status":status,"error":error}),
    )
    .await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MessageQuery {
    head_id: Option<Uuid>,
    limit: Option<i64>,
    cursor: Option<String>,
}
async fn chat_head(app: &Application, owner: Uuid, chat: Uuid) -> Result<Option<Uuid>> {
    sqlx::query_scalar(
        "SELECT head_id FROM chats WHERE id=$1 AND owner_id=$2 AND deleted_at IS NULL",
    )
    .bind(chat)
    .bind(owner)
    .fetch_optional(&app.0.pool)
    .await?
    .ok_or_else(Error::missing)
}
fn message_json(row: &sqlx::postgres::PgRow) -> Value {
    json!({"id":row.get::<Uuid,_>("id"),"parent_id":row.get::<Option<Uuid>,_>("parent_id"),"role":row.get::<String,_>("role"),"parts":row.get::<Value,_>("parts"),"depth":row.get::<i32,_>("depth"),"order":row.get::<i64,_>("seq"),"created_at":row.get::<chrono::DateTime<chrono::Utc>,_>("created_at"),"generation_id":row.get::<Option<Uuid>,_>("generation_id"),"status":row.get::<Option<String>,_>("status")})
}
pub(crate) async fn messages(
    State(app): State<Application>,
    auth: Auth,
    Path(chat): Path<Uuid>,
    Query(q): Query<MessageQuery>,
) -> Result<Json<Value>> {
    let limit = http::limit(q.limit, 50)?;
    let current = chat_head(&app, auth.user, chat).await?;
    let (head, start) = if let Some(s) = q.cursor {
        let c = http::decode(&s, "messages", auth.user, Some(chat))?;
        if q.head_id.is_some() && q.head_id != c.head {
            return Err(Error::bad());
        }
        // A cursor isn't an authorization proof. Verify next belongs to the pinned ancestry.
        let valid: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM messages m JOIN chats c ON c.id=m.chat_id WHERE m.id=$1 AND m.chat_id=$3 AND c.owner_id=$4 AND c.deleted_at IS NULL AND ($2=m.id OR $2=ANY(m.ancestors)))")
            .bind(c.head).bind(c.next).bind(chat).bind(auth.user).fetch_one(&app.0.pool).await?;
        if !valid {
            return Err(Error::bad());
        }
        (c.head, c.next)
    } else {
        let head = q.head_id.or(current);
        (head, head)
    };
    if let Some(head) = head {
        let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM messages m JOIN chats c ON c.id=m.chat_id WHERE m.id=$1 AND m.chat_id=$2 AND c.owner_id=$3 AND c.deleted_at IS NULL)").bind(head).bind(chat).bind(auth.user).fetch_one(&app.0.pool).await?;
        if !exists {
            return Err(Error::missing());
        }
    }
    let mut rows = sqlx::query("WITH RECURSIVE path AS (SELECT m.*,1 AS n FROM messages m WHERE m.id=$1 AND m.chat_id=$2 UNION ALL SELECT m.*,p.n+1 FROM messages m JOIN path p ON m.id=p.parent_id WHERE m.chat_id=$2 AND p.n<$3) SELECT p.*,g.status FROM path p JOIN chats c ON c.id=p.chat_id LEFT JOIN generations g ON g.id=p.generation_id WHERE c.owner_id=$4 AND c.deleted_at IS NULL ORDER BY p.depth DESC")
        .bind(start).bind(chat).bind(limit+1).bind(auth.user).fetch_all(&app.0.pool).await?;
    let next = if rows.len() as i64 > limit {
        let next_id: Uuid = rows.pop().expect("extra row").get("id");
        Some(http::encode(&Cursor {
            v: 1,
            kind: "messages".into(),
            owner: auth.user,
            chat: Some(chat),
            archived: None,
            high: 0,
            before: 0,
            head,
            next: Some(next_id),
        }))
    } else {
        None
    };
    Ok(Json(
        json!({"items":rows.iter().map(message_json).collect::<Vec<_>>(),"head_id":head,"next_cursor":next}),
    ))
}
pub(crate) async fn branches(
    State(app): State<Application>,
    auth: Auth,
    Path(chat): Path<Uuid>,
    Query(q): Query<MessageQuery>,
) -> Result<Json<Value>> {
    if q.head_id.is_some() {
        return Err(Error::bad());
    }
    chat_head(&app, auth.user, chat).await?;
    let limit = http::limit(q.limit, 50)?;
    let (high, before) = if let Some(s) = q.cursor {
        let c = http::decode(&s, "branches", auth.user, Some(chat))?;
        (c.high, c.before)
    } else {
        let mut tx = app.0.pool.begin().await?;
        sqlx::query("SELECT id FROM users WHERE id=$1 FOR UPDATE")
            .bind(auth.user)
            .execute(&mut *tx)
            .await?;
        let high:i64=sqlx::query_scalar("SELECT COALESCE(max(m.seq),0) FROM messages m JOIN chats c ON c.id=m.chat_id WHERE m.chat_id=$1 AND c.owner_id=$2 AND c.deleted_at IS NULL").bind(chat).bind(auth.user).fetch_one(&mut *tx).await?;
        tx.commit().await?;
        (high, high)
    };
    let mut rows=sqlx::query("SELECT m.*,g.status FROM messages m JOIN chats c ON c.id=m.chat_id LEFT JOIN generations g ON g.id=m.generation_id WHERE m.chat_id=$1 AND c.owner_id=$2 AND c.deleted_at IS NULL AND m.seq<=$3 AND m.seq<=$4 AND NOT EXISTS(SELECT 1 FROM messages child WHERE child.chat_id=m.chat_id AND child.parent_id=m.id AND child.seq<=$3) ORDER BY m.seq DESC LIMIT $5")
        .bind(chat).bind(auth.user).bind(high).bind(before).bind(limit+1).fetch_all(&app.0.pool).await?;
    let next = if rows.len() as i64 > limit {
        rows.pop();
        rows.last().map(|r| {
            http::encode(&Cursor {
                v: 1,
                kind: "branches".into(),
                owner: auth.user,
                chat: Some(chat),
                archived: None,
                high,
                before: r.get::<i64, _>("seq") - 1,
                head: None,
                next: None,
            })
        })
    } else {
        None
    };
    Ok(Json(
        json!({"items":rows.iter().map(message_json).collect::<Vec<_>>(),"next_cursor":next}),
    ))
}
pub(crate) async fn generation(
    State(app): State<Application>,
    auth: Auth,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    let row=sqlx::query("SELECT g.* FROM generations g JOIN chats c ON c.id=g.chat_id WHERE g.id=$1 AND g.owner_id=$2 AND c.owner_id=$2 AND c.deleted_at IS NULL").bind(id).bind(auth.user).fetch_optional(&app.0.pool).await?.ok_or_else(Error::missing)?;
    Ok(Json(
        json!({"generation_id":id,"user_message_id":row.get::<Uuid,_>("user_message_id"),"status":row.get::<String,_>("status"),"result_message_id":row.get::<Option<Uuid>,_>("result_message_id"),"error":row.get::<Option<String>,_>("error"),"usage":row.get::<Option<Value>,_>("usage"),"created_at":row.get::<chrono::DateTime<chrono::Utc>,_>("created_at"),"updated_at":row.get::<chrono::DateTime<chrono::Utc>,_>("updated_at")}),
    ))
}
pub(crate) async fn cancel(
    State(app): State<Application>,
    auth: Auth,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    let mut tx = app.0.pool.begin().await?;
    let chat:Uuid=sqlx::query_scalar("SELECT c.id FROM chats c JOIN generations g ON g.chat_id=c.id WHERE g.id=$1 AND g.owner_id=$2 AND c.owner_id=$2 AND c.deleted_at IS NULL FOR UPDATE OF c").bind(id).bind(auth.user).fetch_optional(&mut *tx).await?.ok_or_else(Error::missing)?;
    let status: String = sqlx::query_scalar(
        "SELECT status FROM generations WHERE id=$1 AND owner_id=$2 AND chat_id=$3 FOR UPDATE",
    )
    .bind(id)
    .bind(auth.user)
    .bind(chat)
    .fetch_one(&mut *tx)
    .await?;
    let status = if matches!(status.as_str(), "queued" | "running") {
        terminal(&mut tx, id, "cancelled", None).await?;
        "cancelled".to_owned()
    } else {
        status
    };
    tx.commit().await?;
    Ok(Json(json!({"generation_id":id,"status":status})))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EventsQuery {
    after: Option<i64>,
}
pub(crate) async fn events(
    State(app): State<Application>,
    auth: Auth,
    Path(id): Path<Uuid>,
    Query(q): Query<EventsQuery>,
    headers: HeaderMap,
) -> Result<Response> {
    let _ = generation(
        State(app.clone()),
        Auth {
            user: auth.user,
            hash: auth.hash.clone(),
        },
        Path(id),
    )
    .await?;
    let header_after = headers
        .get("last-event-id")
        .map(|h| {
            h.to_str()
                .ok()
                .and_then(|s| s.parse::<i64>().ok())
                .ok_or_else(Error::bad)
        })
        .transpose()?;
    if q.after.is_some() && header_after.is_some() && q.after != header_after {
        return Err(Error::bad());
    }
    let mut after = q.after.or(header_after).unwrap_or(0);
    if after < 0 {
        return Err(Error::bad());
    }
    let max: i64 =
        sqlx::query_scalar("SELECT next_seq-1 FROM generations WHERE id=$1 AND owner_id=$2")
            .bind(id)
            .bind(auth.user)
            .fetch_one(&app.0.pool)
            .await?;
    if after > max {
        return Err(Error::bad());
    }
    let permit = app
        .0
        .streams
        .clone()
        .try_acquire_owned()
        .map_err(|_| Error(StatusCode::TOO_MANY_REQUESTS, "stream_capacity"))?;
    let stream = async_stream::stream! {
        // Own admission for the lifetime of the response body, not just its headers.
        let _permit=permit;
        loop {
            // Poll authentication/deletion each batch, including while idle. No connection survives yield.
            let state=sqlx::query_scalar::<_,String>("SELECT g.status FROM generations g JOIN chats c ON c.id=g.chat_id JOIN sessions s ON s.user_id=g.owner_id WHERE g.id=$1 AND g.owner_id=$2 AND c.owner_id=$2 AND c.deleted_at IS NULL AND s.token_hash=$3 AND s.expires_at>now()")
                .bind(id).bind(auth.user).bind(&auth.hash).fetch_optional(&app.0.pool).await;
            let status=match state {Ok(Some(s))=>s,Ok(None)=>break,Err(_)=>{yield Err::<Event,std::io::Error>(std::io::Error::other("stream unavailable"));break;}};
            let rows=sqlx::query("SELECT seq,kind,data FROM generation_events WHERE generation_id=$1 AND seq>$2 ORDER BY seq LIMIT 32").bind(id).bind(after).fetch_all(&app.0.pool).await;
            let rows=match rows {Ok(r)=>r,Err(_)=>{yield Err(std::io::Error::other("stream unavailable"));break;}};
            let empty=rows.is_empty();
            for row in rows {
                after=row.get("seq");
                let kind:String=row.get("kind");
                let data:Value=row.get("data");
                yield Ok(Event::default().id(after.to_string()).event(kind.clone()).data(data.to_string()));
                if matches!(kind.as_str(),"completed"|"failed"|"cancelled") {return;}
            }
            if empty && !matches!(status.as_str(),"queued"|"running") {break;}
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
    };
    Ok(Sse::new(stream)
        .keep_alive(KeepAlive::new().interval(std::time::Duration::from_secs(10)))
        .into_response())
}
