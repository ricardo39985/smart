//! Private profile photos: decoded, downscaled, metadata-free PNGs, not chat attachments.
use crate::{
    Application,
    auth::Auth,
    http::{Error, Result},
    identity,
};
use axum::{
    Json,
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use image::ImageDecoder;
use serde_json::Value;
use sqlx::Row;

pub(crate) async fn put(
    State(app): State<Application>,
    auth: Auth,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<Value>> {
    let expected = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(Error::bad)?;
    if !matches!(expected, "image/png" | "image/jpeg" | "image/webp") {
        return Err(Error::bad());
    }
    if body.len() > 2 * 1024 * 1024 {
        return Err(Error(StatusCode::PAYLOAD_TOO_LARGE, "image_too_large"));
    }
    let expected = expected.to_owned();
    let permit = app
        .0
        .images
        .clone()
        .try_acquire_owned()
        .map_err(|_| Error(StatusCode::TOO_MANY_REQUESTS, "image_decoder_busy"))?;
    let bytes = tokio::task::spawn_blocking(move || -> Result<Vec<u8>> {
        let _permit = permit;
        let format = image::guess_format(&body).map_err(|_| Error::bad())?;
        let mime = match format {
            image::ImageFormat::Png => "image/png",
            image::ImageFormat::Jpeg => "image/jpeg",
            image::ImageFormat::WebP => "image/webp",
            _ => return Err(Error::bad()),
        };
        if expected != mime {
            return Err(Error::bad());
        }
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(4096);
        limits.max_image_height = Some(4096);
        limits.max_alloc = Some(128 * 1024 * 1024);
        let mut reader = image::ImageReader::with_format(std::io::Cursor::new(&body), format);
        reader.limits(limits.clone());
        let mut decoder = reader.into_decoder().map_err(|_| Error::bad())?;
        let (w, h) = decoder.dimensions();
        if w == 0 || h == 0 || u64::from(w) * u64::from(h) > 16_000_000 {
            return Err(Error::bad());
        }
        // Keep ImageReader::decode's output-buffer reservation when using the
        // decoder directly to read orientation before discarding metadata.
        limits
            .reserve(decoder.total_bytes())
            .map_err(|_| Error::bad())?;
        decoder.set_limits(limits).map_err(|_| Error::bad())?;
        let orientation = decoder.orientation().map_err(|_| Error::bad())?;
        let mut image = image::DynamicImage::from_decoder(decoder).map_err(|_| Error::bad())?;
        image.apply_orientation(orientation);
        let image = if w > 512 || h > 512 {
            image.resize(512, 512, image::imageops::FilterType::Lanczos3)
        } else {
            image
        };
        let mut output = std::io::Cursor::new(Vec::new());
        // Construct from pixels only, not the decoded container's metadata/EXIF.
        image::DynamicImage::ImageRgba8(image.to_rgba8())
            .write_to(&mut output, image::ImageFormat::Png)
            .map_err(|_| Error::bad())?;
        let output = output.into_inner();
        if output.len() > 2 * 1024 * 1024 {
            return Err(Error(StatusCode::PAYLOAD_TOO_LARGE, "image_too_large"));
        }
        Ok(output)
    })
    .await
    .map_err(|_| Error::unavailable())??;
    let mut tx = app.0.pool.begin().await?;
    sqlx::query("INSERT INTO user_profile_photos(user_id,bytes) VALUES($1,$2) ON CONFLICT (user_id) DO UPDATE SET bytes=EXCLUDED.bytes")
        .bind(auth.user).bind(bytes).execute(&mut *tx).await?;
    let account = identity::account_value(&mut tx, auth.user).await?;
    tx.commit().await?;
    Ok(Json(account))
}

pub(crate) async fn get(State(app): State<Application>, auth: Auth) -> Result<Response> {
    let row = sqlx::query("SELECT bytes FROM user_profile_photos WHERE user_id=$1")
        .bind(auth.user)
        .fetch_optional(&app.0.pool)
        .await?
        .ok_or_else(Error::missing)?;
    let bytes: Vec<u8> = row.get("bytes");
    Ok((
        [
            (header::CONTENT_TYPE, "image/png"),
            (header::CACHE_CONTROL, "no-store"),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        ],
        bytes,
    )
        .into_response())
}

pub(crate) async fn remove(State(app): State<Application>, auth: Auth) -> Result<StatusCode> {
    sqlx::query("DELETE FROM user_profile_photos WHERE user_id=$1")
        .bind(auth.user)
        .execute(&app.0.pool)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
