use crate::{
    Application,
    auth::Auth,
    http::{Error, Result},
};
use axum::{
    Json,
    body::Bytes,
    extract::{Path, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};
use sqlx::Row;
use uuid::Uuid;

pub(crate) struct ValidatedImage {
    pub bytes: Vec<u8>,
    pub mime: &'static str,
    pub width: u32,
    pub height: u32,
}
pub(crate) async fn validate(
    app: &Application,
    bytes: Vec<u8>,
    expected: Option<String>,
) -> Result<ValidatedImage> {
    if bytes.is_empty() || bytes.len() > 8 * 1024 * 1024 {
        return Err(Error(StatusCode::PAYLOAD_TOO_LARGE, "image_too_large"));
    }
    let permit = app
        .0
        .images
        .clone()
        .try_acquire_owned()
        .map_err(|_| Error(StatusCode::TOO_MANY_REQUESTS, "image_decoder_busy"))?;
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let format = image::guess_format(&bytes).map_err(|_| Error::bad())?;
        let mime = match format {
            image::ImageFormat::Jpeg => "image/jpeg",
            image::ImageFormat::Png => "image/png",
            image::ImageFormat::WebP => "image/webp",
            _ => return Err(Error::bad()),
        };
        if expected.as_deref().is_some_and(|e| e != mime) {
            return Err(Error::bad());
        }
        let mut reader = image::ImageReader::with_format(std::io::Cursor::new(&bytes), format);
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(20000);
        limits.max_image_height = Some(20000);
        limits.max_alloc = Some(160 * 1024 * 1024);
        reader.limits(limits.clone());
        let (width, height) = reader.into_dimensions().map_err(|_| Error::bad())?;
        if u64::from(width) * u64::from(height) > 20_000_000 || width == 0 || height == 0 {
            return Err(Error::bad());
        }
        let mut reader = image::ImageReader::with_format(std::io::Cursor::new(&bytes), format);
        reader.limits(limits);
        reader.decode().map_err(|_| Error::bad())?;
        Ok(ValidatedImage {
            bytes,
            mime,
            width,
            height,
        })
    })
    .await
    .map_err(|_| Error::unavailable())?
}
pub(crate) fn metadata(id: Uuid, mime: &str, width: i32, height: i32) -> Value {
    json!({"id":id,"url":format!("/v1/attachments/{id}"),"mime":mime,"width":width,"height":height})
}
pub(crate) async fn store(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    owner: Uuid,
    image: ValidatedImage,
    attached: bool,
) -> Result<Value> {
    let n = image.bytes.len() as i64;
    let changed = sqlx::query(
        "UPDATE users SET image_bytes=image_bytes+$2 WHERE id=$1 AND image_bytes+$2<=67108864",
    )
    .bind(owner)
    .bind(n)
    .execute(&mut **tx)
    .await?
    .rows_affected();
    if changed == 0 {
        return Err(Error(StatusCode::PAYLOAD_TOO_LARGE, "image_quota_exceeded"));
    }
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO attachments(id,owner_id,bytes,mime,width,height,ever_attached) VALUES($1,$2,$3,$4,$5,$6,$7)")
        .bind(id).bind(owner).bind(image.bytes).bind(image.mime).bind(image.width as i32).bind(image.height as i32).bind(attached).execute(&mut **tx).await?;
    Ok(metadata(
        id,
        image.mime,
        image.width as i32,
        image.height as i32,
    ))
}
pub(crate) async fn upload(
    State(app): State<Application>,
    auth: Auth,
    headers: HeaderMap,
    body: Bytes,
) -> Result<(StatusCode, Json<Value>)> {
    let mime = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(Error::bad)?
        .to_owned();
    if !matches!(mime.as_str(), "image/jpeg" | "image/png" | "image/webp") {
        return Err(Error::bad());
    }
    let image = validate(&app, body.to_vec(), Some(mime)).await?;
    let mut tx = app.0.pool.begin().await?;
    let meta = store(&mut tx, auth.user, image, false).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(meta)))
}
pub(crate) async fn download(
    State(app): State<Application>,
    auth: Auth,
    Path(id): Path<Uuid>,
) -> Result<Response> {
    let row = sqlx::query("SELECT a.bytes,a.mime FROM attachments a WHERE a.id=$1 AND a.owner_id=$2 AND (NOT a.ever_attached OR EXISTS(SELECT 1 FROM message_attachments ma JOIN messages m ON m.id=ma.message_id JOIN chats c ON c.id=m.chat_id WHERE ma.attachment_id=a.id AND c.owner_id=$2 AND c.deleted_at IS NULL))")
        .bind(id).bind(auth.user).fetch_optional(&app.0.pool).await?.ok_or_else(Error::missing)?;
    let mime: String = row.get("mime");
    let bytes: Vec<u8> = row.get("bytes");
    Ok((
        [
            (header::CONTENT_TYPE, mime),
            (header::CACHE_CONTROL, "no-store".to_owned()),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff".to_owned()),
        ],
        bytes,
    )
        .into_response())
}
pub(crate) async fn remove(
    State(app): State<Application>,
    auth: Auth,
    Path(id): Path<Uuid>,
) -> Result<StatusCode> {
    let mut tx = app.0.pool.begin().await?;
    sqlx::query("SELECT id FROM users WHERE id=$1 FOR UPDATE")
        .bind(auth.user)
        .execute(&mut *tx)
        .await?;
    let row = sqlx::query("SELECT octet_length(bytes)::bigint AS size,ever_attached FROM attachments WHERE id=$1 AND owner_id=$2 FOR UPDATE").bind(id).bind(auth.user).fetch_optional(&mut *tx).await?.ok_or_else(Error::missing)?;
    if row.get::<bool, _>("ever_attached") {
        return Err(Error::conflict());
    }
    sqlx::query("DELETE FROM attachments WHERE id=$1 AND owner_id=$2")
        .bind(id)
        .bind(auth.user)
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE users SET image_bytes=image_bytes-$2 WHERE id=$1")
        .bind(auth.user)
        .bind(row.get::<i64, _>("size"))
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
