use crate::{
    Application,
    http::{Error, Result},
};
use argon2::{
    Argon2, PasswordHasher, PasswordVerifier,
    password_hash::{PasswordHash, SaltString},
};
use axum::{
    Json,
    extract::{FromRequestParts, State},
    http::{HeaderMap, StatusCode, request::Parts},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::RngCore;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{Postgres, Row, Transaction};
use uuid::Uuid;

pub(crate) struct Auth {
    pub user: Uuid,
    pub hash: Vec<u8>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}
impl FromRequestParts<Application> for Auth {
    type Rejection = Error;
    async fn from_request_parts(parts: &mut Parts, app: &Application) -> Result<Self> {
        authenticate(app, &parts.headers).await
    }
}
pub(crate) async fn authenticate(app: &Application, headers: &HeaderMap) -> Result<Auth> {
    let token = headers
        .get("authorization")
        .and_then(|s| s.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .filter(|s| s.len() == 43)
        .ok_or(Error(StatusCode::UNAUTHORIZED, "unauthorized"))?;
    let hash = Sha256::digest(token.as_bytes()).to_vec();
    let row = sqlx::query(
        "SELECT user_id,created_at FROM sessions WHERE token_hash=$1 AND expires_at>now()",
    )
    .bind(&hash)
    .fetch_optional(&app.0.pool)
    .await?
    .ok_or(Error(StatusCode::UNAUTHORIZED, "unauthorized"))?;
    Ok(Auth {
        user: row.get("user_id"),
        hash,
        created_at: row.get("created_at"),
    })
}
pub(crate) fn hash_password(password: String) -> anyhow::Result<String> {
    let salt = SaltString::generate(&mut rand::rngs::OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|_| anyhow::anyhow!("password hashing failed"))
}
pub(crate) fn email(s: &str) -> Option<String> {
    let s = s.trim().to_lowercase();
    let (local, domain) = s.split_once('@')?;
    (s.len() <= 254
        && !local.is_empty()
        && !domain.is_empty()
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && domain.contains('.')
        && !s.contains(char::is_whitespace)
        && !s.chars().any(char::is_control)
        && !domain.contains('@')
        && !local.contains(' ')
        && !s.contains(".."))
    .then_some(s)
}
pub(crate) fn throttle(app: &Application) -> Result<()> {
    let mut w = app
        .0
        .login_window
        .lock()
        .map_err(|_| Error::unavailable())?;
    if w.0.elapsed().as_secs() >= 1 {
        *w = (std::time::Instant::now(), 0);
    }
    if w.1 >= 16 {
        return Err(Error(StatusCode::TOO_MANY_REQUESTS, "rate_limited"));
    }
    w.1 += 1;
    Ok(())
}
pub(crate) async fn verify(app: &Application, user: Uuid, password: String) -> Result<bool> {
    if password.len() > 1024 {
        return Err(Error::bad());
    }
    let permit = app
        .0
        .hashes
        .clone()
        .try_acquire_owned()
        .map_err(|_| Error(StatusCode::TOO_MANY_REQUESTS, "rate_limited"))?;
    let row = sqlx::query("SELECT password_hash FROM users WHERE id=$1")
        .bind(user)
        .fetch_optional(&app.0.pool)
        .await?;
    let hash: String = row
        .and_then(|r| r.get::<Option<String>, _>("password_hash"))
        .unwrap_or_else(|| app.0.dummy_hash.clone());
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        PasswordHash::new(&hash).ok().is_some_and(|h| {
            Argon2::default()
                .verify_password(password.as_bytes(), &h)
                .is_ok()
        })
    })
    .await
    .map_err(|_| Error::unavailable())
}
pub(crate) async fn issue(tx: &mut Transaction<'_, Postgres>, user: Uuid) -> Result<Value> {
    let mut bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    let token = URL_SAFE_NO_PAD.encode(bytes);
    let expires = chrono::Utc::now() + chrono::Duration::days(30);
    sqlx::query("SELECT id FROM users WHERE id=$1 FOR UPDATE")
        .bind(user)
        .execute(&mut **tx)
        .await?;
    sqlx::query("DELETE FROM sessions WHERE user_id=$1 AND (expires_at<=now() OR token_hash IN (SELECT token_hash FROM sessions WHERE user_id=$1 ORDER BY expires_at DESC OFFSET 15))")
        .bind(user).execute(&mut **tx).await?;
    sqlx::query("INSERT INTO sessions(token_hash,user_id,expires_at) VALUES($1,$2,$3)")
        .bind(Sha256::digest(token.as_bytes()).to_vec())
        .bind(user)
        .bind(expires)
        .execute(&mut **tx)
        .await?;
    Ok(json!({"token":token,"expires_at":expires,"user_id":user}))
}
pub(crate) async fn provision(
    app: &Application,
    address: &str,
    password: String,
) -> anyhow::Result<Uuid> {
    let email = email(address).ok_or_else(|| anyhow::anyhow!("invalid email"))?;
    anyhow::ensure!(
        (12..=1024).contains(&password.len()),
        "password must be 12..=1024 bytes"
    );
    let _permit = app.0.hashes.clone().acquire_owned().await?;
    let hash = tokio::task::spawn_blocking(move || hash_password(password)).await??;
    let id = Uuid::new_v4();
    let display_name: String = email
        .split('@')
        .next()
        .unwrap_or("User")
        .chars()
        .take(80)
        .collect();
    sqlx::query("INSERT INTO users(id,email,password_hash,display_name) VALUES($1,$2,$3,$4)")
        .bind(id)
        .bind(email)
        .bind(hash)
        .bind(display_name)
        .execute(&app.0.pool)
        .await?;
    Ok(id)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Login {
    email: String,
    password: String,
}
pub(crate) async fn login(
    State(app): State<Application>,
    Json(input): Json<Login>,
) -> Result<Json<Value>> {
    if input.password.len() > 1024 || input.email.len() > 254 {
        return Err(Error::bad());
    }
    throttle(&app)?;
    let permit = app
        .0
        .hashes
        .clone()
        .try_acquire_owned()
        .map_err(|_| Error(StatusCode::TOO_MANY_REQUESTS, "rate_limited"))?;
    let row = sqlx::query("SELECT id,password_hash FROM users WHERE email=$1")
        .bind(email(&input.email).unwrap_or_default())
        .fetch_optional(&app.0.pool)
        .await?;
    let hash: String = row
        .as_ref()
        .and_then(|r| r.get::<Option<String>, _>("password_hash"))
        .unwrap_or_else(|| app.0.dummy_hash.clone());
    let valid = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        PasswordHash::new(&hash).ok().is_some_and(|hash| {
            Argon2::default()
                .verify_password(input.password.as_bytes(), &hash)
                .is_ok()
        })
    })
    .await
    .map_err(|_| Error::unavailable())?;
    let row = row
        .filter(|r| valid && r.get::<Option<String>, _>("password_hash").is_some())
        .ok_or(Error(StatusCode::UNAUTHORIZED, "unauthorized"))?;
    let user: Uuid = row.get("id");
    let mut tx = app.0.pool.begin().await?;
    let session = issue(&mut tx, user).await?;
    tx.commit().await?;
    Ok(Json(session))
}
pub(crate) async fn logout(
    State(app): State<Application>,
    auth: Auth,
) -> Result<axum::http::StatusCode> {
    sqlx::query("DELETE FROM sessions WHERE token_hash=$1 AND user_id=$2")
        .bind(auth.hash)
        .bind(auth.user)
        .execute(&app.0.pool)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
