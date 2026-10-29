//! Projects organize owned chats; deleting a project never deletes its conversations.
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
use sqlx::{FromRow, Row};
use uuid::Uuid;

#[derive(Serialize, FromRow)]
struct Project {
    id: Uuid,
    name: String,
    version: i64,
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
    #[serde(skip)]
    seq: i64,
}

fn name_valid(name: &str) -> bool {
    !name.trim().is_empty() && name.chars().count() <= 200
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Create {
    request_id: Uuid,
    name: String,
}

pub(crate) async fn create(
    State(app): State<Application>,
    auth: Auth,
    Json(input): Json<Create>,
) -> Result<(StatusCode, Json<Value>)> {
    if !name_valid(&input.name) {
        return Err(Error::bad());
    }
    let fingerprint = Sha256::digest(input.name.as_bytes()).to_vec();
    let mut tx = app.0.pool.begin().await?;
    sqlx::query("SELECT id FROM users WHERE id=$1 FOR UPDATE")
        .bind(auth.user)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(Error::missing)?;
    if let Some(row) = sqlx::query("SELECT id,name,version,created_at,updated_at,seq,fingerprint,deleted_at FROM projects WHERE owner_id=$1 AND request_id=$2")
        .bind(auth.user)
        .bind(input.request_id)
        .fetch_optional(&mut *tx)
        .await?
    {
        if row.get::<Vec<u8>, _>("fingerprint") != fingerprint {
            return Err(Error::conflict());
        }
        if row
            .get::<Option<chrono::DateTime<chrono::Utc>>, _>("deleted_at")
            .is_some()
        {
            return Err(Error(StatusCode::GONE, "project_deleted"));
        }
        let project = Project::from_row(&row)?;
        tx.commit().await?;
        return Ok((StatusCode::OK, Json(json!(project))));
    }
    let active: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM projects WHERE owner_id=$1 AND deleted_at IS NULL",
    )
    .bind(auth.user)
    .fetch_one(&mut *tx)
    .await?;
    if active >= 1000 {
        return Err(Error(StatusCode::TOO_MANY_REQUESTS, "project_limit"));
    }
    let project = sqlx::query_as::<_, Project>("INSERT INTO projects(id,owner_id,name,request_id,fingerprint) VALUES($1,$2,$3,$4,$5) RETURNING id,name,version,created_at,updated_at,seq")
        .bind(Uuid::new_v4()).bind(auth.user).bind(input.name).bind(input.request_id).bind(fingerprint)
        .fetch_one(&mut *tx).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(json!(project))))
}

pub(crate) async fn get(
    State(app): State<Application>,
    auth: Auth,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    let project = sqlx::query_as::<_, Project>(
        "SELECT id,name,version,created_at,updated_at,seq FROM projects WHERE id=$1 AND owner_id=$2 AND deleted_at IS NULL",
    )
    .bind(id)
    .bind(auth.user)
    .fetch_optional(&app.0.pool)
    .await?
    .ok_or_else(Error::missing)?;
    Ok(Json(json!(project)))
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
        if encoded.len() > 1024 {
            return Err(Error::bad());
        }
        let cursor: Cursor =
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(encoded).map_err(|_| Error::bad())?)
                .map_err(|_| Error::bad())?;
        if cursor.v != 1
            || cursor.kind != "projects"
            || cursor.owner != auth.user
            || cursor.high < 0
            || cursor.before < 0
            || cursor.before > cursor.high
        {
            return Err(Error::bad());
        }
        (cursor.high, cursor.before)
    } else {
        let mut tx = app.0.pool.begin().await?;
        // Creation uses the same owner lock so late commits cannot enter an existing page.
        sqlx::query("SELECT id FROM users WHERE id=$1 FOR UPDATE")
            .bind(auth.user)
            .execute(&mut *tx)
            .await?;
        let high: i64 = sqlx::query_scalar(
            "SELECT COALESCE(max(seq),0) FROM projects WHERE owner_id=$1 AND deleted_at IS NULL",
        )
        .bind(auth.user)
        .fetch_one(&mut *tx)
        .await?;
        tx.commit().await?;
        (high, high)
    };
    let mut projects = sqlx::query_as::<_, Project>("SELECT id,name,version,created_at,updated_at,seq FROM projects WHERE owner_id=$1 AND deleted_at IS NULL AND seq<=$2 AND seq<=$3 ORDER BY seq DESC LIMIT $4")
        .bind(auth.user).bind(high).bind(before).bind(page_size+1).fetch_all(&app.0.pool).await?;
    let next_cursor = if projects.len() as i64 > page_size {
        projects.pop();
        projects.last().map(|project| {
            URL_SAFE_NO_PAD.encode(
                serde_json::to_vec(&Cursor {
                    v: 1,
                    kind: "projects".into(),
                    owner: auth.user,
                    high,
                    before: project.seq - 1,
                })
                .expect("serializable project cursor"),
            )
        })
    } else {
        None
    };
    Ok(Json(json!({"items":projects,"next_cursor":next_cursor})))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Update {
    version: i64,
    name: String,
}

pub(crate) async fn update(
    State(app): State<Application>,
    auth: Auth,
    Path(id): Path<Uuid>,
    Json(input): Json<Update>,
) -> Result<Json<Value>> {
    if input.version < 0 || !name_valid(&input.name) {
        return Err(Error::bad());
    }
    let mut tx = app.0.pool.begin().await?;
    sqlx::query("SELECT id FROM users WHERE id=$1 FOR UPDATE")
        .bind(auth.user)
        .execute(&mut *tx)
        .await?;
    let old: i64 = sqlx::query_scalar("SELECT version FROM projects WHERE id=$1 AND owner_id=$2 AND deleted_at IS NULL FOR UPDATE")
        .bind(id).bind(auth.user).fetch_optional(&mut *tx).await?.ok_or_else(Error::missing)?;
    if old != input.version {
        return Err(Error::conflict());
    }
    let project = sqlx::query_as::<_,Project>("UPDATE projects SET name=$3,version=version+1,updated_at=now() WHERE id=$1 AND owner_id=$2 RETURNING id,name,version,created_at,updated_at,seq")
        .bind(id).bind(auth.user).bind(input.name).fetch_one(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(json!(project)))
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
    Query(input): Query<Version>,
) -> Result<StatusCode> {
    if input.version < 0 {
        return Err(Error::bad());
    }
    let mut tx = app.0.pool.begin().await?;
    sqlx::query("SELECT id FROM users WHERE id=$1 FOR UPDATE")
        .bind(auth.user)
        .execute(&mut *tx)
        .await?;
    let old: i64 = sqlx::query_scalar("SELECT version FROM projects WHERE id=$1 AND owner_id=$2 AND deleted_at IS NULL FOR UPDATE")
        .bind(id).bind(auth.user).fetch_optional(&mut *tx).await?.ok_or_else(Error::missing)?;
    if old != input.version {
        return Err(Error::conflict());
    }
    sqlx::query("UPDATE chats SET project_id=NULL,version=version+1,updated_at=now() WHERE owner_id=$1 AND project_id=$2")
        .bind(auth.user).bind(id).execute(&mut *tx).await?;
    sqlx::query("UPDATE projects SET deleted_at=now(),version=version+1,updated_at=now() WHERE id=$1 AND owner_id=$2")
        .bind(id).bind(auth.user).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

// A non-Option field makes omitted project_id invalid, while unit accepts explicit null.
#[derive(Deserialize)]
#[serde(untagged)]
enum Destination {
    Project(Uuid),
    None(()),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Move {
    version: i64,
    project_id: Destination,
}

pub(crate) async fn move_chat(
    State(app): State<Application>,
    auth: Auth,
    Path(id): Path<Uuid>,
    Json(input): Json<Move>,
) -> Result<Json<Value>> {
    if input.version < 0 {
        return Err(Error::bad());
    }
    let project = match input.project_id {
        Destination::Project(id) => Some(id),
        Destination::None(()) => None,
    };
    let mut tx = app.0.pool.begin().await?;
    sqlx::query("SELECT id FROM users WHERE id=$1 FOR UPDATE")
        .bind(auth.user)
        .execute(&mut *tx)
        .await?;
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
    if let Some(project) = project {
        sqlx::query("SELECT id FROM projects WHERE id=$1 AND owner_id=$2 AND deleted_at IS NULL")
            .bind(project)
            .bind(auth.user)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(Error::missing)?;
    }
    let version: i64 = sqlx::query_scalar("UPDATE chats SET project_id=$3,version=version+1,updated_at=now() WHERE id=$1 AND owner_id=$2 RETURNING version")
        .bind(id).bind(auth.user).bind(project).fetch_one(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(
        json!({"id":id,"version":version,"project_id":project}),
    ))
}
