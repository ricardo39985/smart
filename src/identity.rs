//! Native proof-bound identity acquisition. Only a verified exchange mutates accounts.
use crate::{
    Application, IdentityConfig,
    auth::{self, Auth},
    http::{Error, Result},
};
use axum::{
    Json,
    extract::{Form, Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use jsonwebtoken::{
    Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode, jwk::JwkSet,
};
use rand::RngCore;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::Row;
use std::time::{Duration, Instant};
use uuid::Uuid;

fn random() -> String {
    let mut b = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut b);
    URL_SAFE_NO_PAD.encode(b)
}
fn digest(s: &str) -> Vec<u8> {
    Sha256::digest(s.as_bytes()).to_vec()
}
fn provider(s: &str) -> Result<&str> {
    match s {
        "google" | "apple" => Ok(s),
        _ => Err(Error::missing()),
    }
}
fn configured<'a>(config: &'a IdentityConfig, p: &str) -> Result<&'a str> {
    match p {
        "google" => config.google.as_ref().map(|g| g.client_id.as_str()),
        "apple" => config.apple.as_ref().map(|a| a.client_id.as_str()),
        _ => return Err(Error::missing()),
    }
    .ok_or(Error(
        StatusCode::SERVICE_UNAVAILABLE,
        "provider_unavailable",
    ))
}
pub(crate) fn validate_config(c: &IdentityConfig) -> anyhow::Result<()> {
    if let Some(base) = &c.local_simulator {
        let u = reqwest::Url::parse(base)?;
        anyhow::ensure!(
            u.scheme() == "http"
                && matches!(u.host_str(), Some("127.0.0.1" | "localhost" | "::1"))
                && u.username().is_empty()
                && u.password().is_none()
                && u.path() == "/"
                && u.query().is_none()
                && u.fragment().is_none(),
            "identity simulator must be loopback HTTP origin"
        );
    }
    if c.google.is_some() || c.apple.is_some() || c.origin.is_some() || c.app_scheme.is_some() {
        let origin = c
            .origin
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("missing auth origin"))?;
        anyhow::ensure!(
            matches!(
                origin,
                "https://dev.smartcindy.app" | "https://smartcindy.app"
            ),
            "invalid auth origin"
        );
        anyhow::ensure!(
            c.app_scheme.as_deref()
                == Some(if origin == "https://dev.smartcindy.app" {
                    "smartcindy-dev"
                } else {
                    "smartcindy"
                }),
            "auth scheme mismatch"
        );
        if let Some(g) = &c.google {
            anyhow::ensure!(
                !g.client_id.is_empty() && !g.client_secret.is_empty(),
                "incomplete google configuration"
            );
        }
        if let Some(a) = &c.apple {
            anyhow::ensure!(
                !a.client_id.is_empty()
                    && !a.team_id.is_empty()
                    && !a.key_id.is_empty()
                    && !a.private_key.is_empty(),
                "incomplete apple configuration"
            );
            EncodingKey::from_ec_pem(a.private_key.as_bytes())?;
        }
    }
    Ok(())
}
fn endpoint(app: &Application, p: &str, path: &str) -> String {
    if let Some(base) = &app.0.config.identity.local_simulator {
        return format!("{base}{p}/{path}");
    }
    match (p, path) {
        ("google", "authorize") => "https://accounts.google.com/o/oauth2/v2/auth".into(),
        ("google", "token") => "https://oauth2.googleapis.com/token".into(),
        ("google", "keys") => "https://www.googleapis.com/oauth2/v3/certs".into(),
        ("apple", "authorize") => "https://appleid.apple.com/auth/authorize".into(),
        ("apple", "token") => "https://appleid.apple.com/auth/token".into(),
        ("apple", "keys") => "https://appleid.apple.com/auth/keys".into(),
        _ => unreachable!(),
    }
}
fn callback_url(app: &Application, p: &str) -> Result<String> {
    Ok(format!(
        "{}/v1/auth/{p}/callback",
        app.0
            .config
            .identity
            .origin
            .as_deref()
            .ok_or(Error::unavailable())?
    ))
}
fn name(s: &str) -> Result<String> {
    let s = s.trim();
    if (1..=80).contains(&s.chars().count()) && !s.chars().any(char::is_control) {
        Ok(s.into())
    } else {
        Err(Error::bad())
    }
}
fn picture(s: &str) -> Option<String> {
    if s.len() > 2048 || s.chars().any(char::is_control) {
        return None;
    }
    let url = reqwest::Url::parse(s).ok()?;
    let host = url.host_str()?;
    (url.scheme() == "https"
        && (host == "googleusercontent.com" || host.ends_with(".googleusercontent.com"))
        && url.username().is_empty()
        && url.password().is_none()
        && url.port().is_none()
        && url.fragment().is_none())
    .then(|| s.to_owned())
}
// Serialize signups/new provider accounts for the same email, including currently
// verified provider email aliases. This never authorizes linking by address.
async fn reserve_email(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    email: &str,
) -> Result<bool> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(email)
        .execute(&mut **tx)
        .await?;
    let claimed: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE email=$1 UNION ALL SELECT 1 FROM account_identities WHERE verified_email=$1)")
        .bind(email).fetch_one(&mut **tx).await?;
    Ok(!claimed)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Signup {
    email: String,
    password: String,
    display_name: String,
}
pub(crate) async fn signup(
    State(app): State<Application>,
    Json(input): Json<Signup>,
) -> Result<Response> {
    auth::throttle(&app)?;
    let email = auth::email(&input.email).ok_or(Error::bad())?;
    let display_name = name(&input.display_name)?;
    if !(12..=1024).contains(&input.password.len()) {
        return Err(Error::bad());
    }
    let permit = app
        .0
        .hashes
        .clone()
        .try_acquire_owned()
        .map_err(|_| Error(StatusCode::TOO_MANY_REQUESTS, "rate_limited"))?;
    let hash = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        auth::hash_password(input.password)
    })
    .await
    .map_err(|_| Error::unavailable())?
    .map_err(|_| Error::unavailable())?;
    let mut tx = app.0.pool.begin().await?;
    let id = Uuid::new_v4();
    if !reserve_email(&mut tx, &email).await? {
        return Err(Error(StatusCode::CONFLICT, "account_exists"));
    }
    let result =
        sqlx::query("INSERT INTO users(id,email,password_hash,display_name) VALUES($1,$2,$3,$4)")
            .bind(id)
            .bind(email)
            .bind(hash)
            .bind(display_name)
            .execute(&mut *tx)
            .await;
    if let Err(e) = result {
        return Err(if unique(&e) {
            Error(StatusCode::CONFLICT, "account_exists")
        } else {
            e.into()
        });
    }
    let session = auth::issue(&mut tx, id).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(session)).into_response())
}
fn unique(e: &sqlx::Error) -> bool {
    matches!(e,sqlx::Error::Database(d) if d.code().as_deref()==Some("23505"))
}
pub(crate) async fn account_value(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    user: Uuid,
) -> Result<Value> {
    let row = sqlx::query(
        "SELECT email,display_name,name_source,password_hash IS NOT NULL AS password FROM users WHERE id=$1",
    )
    .bind(user)
    .fetch_one(&mut **tx)
    .await?;
    let providers:Vec<String>=sqlx::query_scalar("SELECT provider FROM account_identities WHERE user_id=$1 ORDER BY CASE provider WHEN 'google' THEN 0 ELSE 1 END")
        .bind(user).fetch_all(&mut **tx).await?;
    let mut methods = Vec::new();
    if row.get::<bool, _>("password") {
        methods.push("password");
    }
    for p in &providers {
        methods.push(p);
    }
    let source: String = row.get("name_source");
    let identities = sqlx::query("SELECT provider,verified_email,picture_url FROM account_identities WHERE user_id=$1 ORDER BY CASE provider WHEN 'google' THEN 0 ELSE 1 END")
        .bind(user).fetch_all(&mut **tx).await?;
    // Email changes require a future verified email-delivery flow. Provider login
    // metadata is never the account's editable/displayed email authority.
    let avatar_url = identities
        .iter()
        .find(|i| i.get::<String, _>("provider") == "google")
        .and_then(|i| i.get::<Option<String>, _>("picture_url"));
    let custom: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM user_profile_photos WHERE user_id=$1)")
            .bind(user)
            .fetch_one(&mut **tx)
            .await?;
    Ok(
        json!({"id":user,"email":row.get::<String,_>("email"),"display_name":row.get::<String,_>("display_name"),"display_name_source":source,"avatar_url":avatar_url,"has_custom_avatar":custom,"methods":methods,"capabilities":{"email_change":false,"password_change":false}}),
    )
}
pub(crate) async fn account(State(app): State<Application>, auth: Auth) -> Result<Json<Value>> {
    let mut tx = app.0.pool.begin().await?;
    let v = account_value(&mut tx, auth.user).await?;
    Ok(Json(v))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Profile {
    display_name: String,
}
pub(crate) async fn profile(
    State(app): State<Application>,
    auth: Auth,
    Json(input): Json<Profile>,
) -> Result<Json<Value>> {
    let name = name(&input.display_name)?;
    let mut tx = app.0.pool.begin().await?;
    sqlx::query("UPDATE users SET display_name=$1,name_source='local' WHERE id=$2")
        .bind(name)
        .bind(auth.user)
        .execute(&mut *tx)
        .await?;
    let value = account_value(&mut tx, auth.user).await?;
    tx.commit().await?;
    Ok(Json(value))
}
/// Authenticated, intentionally disabled until verified email delivery exists.
/// No request credentials are parsed, persisted or acted on.
pub(crate) async fn email_change_disabled(_auth: Auth) -> Result<StatusCode> {
    Err(Error(
        StatusCode::SERVICE_UNAVAILABLE,
        "email_delivery_unavailable",
    ))
}

/// Password replacement and adding a password to a provider account both require
/// the same future proof flow. Never report a fake successful mutation.
pub(crate) async fn password_change_disabled(_auth: Auth) -> Result<StatusCode> {
    Err(Error(
        StatusCode::SERVICE_UNAVAILABLE,
        "email_delivery_unavailable",
    ))
}

pub(crate) async fn methods(State(app): State<Application>) -> Json<Value> {
    Json(
        json!({"password":true,"google":app.0.config.identity.google.is_some(),"apple":app.0.config.identity.apple.is_some()}),
    )
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Start {
    challenge: String,
    #[serde(default)]
    link: bool,
    #[serde(default)]
    confirm: bool,
    password: Option<String>,
}
pub(crate) async fn start(
    State(app): State<Application>,
    Path(p): Path<String>,
    headers: HeaderMap,
    Json(input): Json<Start>,
) -> Result<Json<Value>> {
    let p = provider(&p)?;
    auth::throttle(&app)?;
    let id_client = configured(&app.0.config.identity, p)?;
    if input.challenge.len() != 43
        || URL_SAFE_NO_PAD
            .decode(&input.challenge)
            .map_err(|_| Error::bad())?
            .len()
            != 32
    {
        return Err(Error::bad());
    }
    let (owner, owner_session_hash) = if input.link {
        if !input.confirm {
            return Err(Error::bad());
        }
        let auth = auth::authenticate(&app, &headers).await?;
        let row =
            sqlx::query("SELECT password_hash IS NOT NULL AS password FROM users WHERE id=$1")
                .bind(auth.user)
                .fetch_one(&app.0.pool)
                .await?;
        if row.get::<bool, _>("password") {
            if !auth::verify(
                &app,
                auth.user,
                input
                    .password
                    .ok_or(Error(StatusCode::UNAUTHORIZED, "unauthorized"))?,
            )
            .await?
            {
                return Err(Error(StatusCode::UNAUTHORIZED, "unauthorized"));
            }
        } else if auth.created_at < chrono::Utc::now() - chrono::Duration::minutes(5) {
            return Err(Error(StatusCode::UNAUTHORIZED, "unauthorized"));
        }
        (Some(auth.user), Some(auth.hash))
    } else {
        if input.confirm || input.password.is_some() {
            return Err(Error::bad());
        }
        (None, None)
    };
    let id = Uuid::new_v4();
    let state = random();
    let nonce = random();
    let verifier = if p == "google" { Some(random()) } else { None };
    sqlx::query("DELETE FROM auth_attempts WHERE id IN (SELECT id FROM auth_attempts WHERE expires_at<now() ORDER BY expires_at LIMIT 100 FOR UPDATE SKIP LOCKED)")
        .execute(&app.0.pool).await?;
    sqlx::query("INSERT INTO auth_attempts(id,provider,state_hash,nonce,challenge,code_verifier,owner,owner_session_hash,expires_at) VALUES($1,$2,$3,$4,$5,$6,$7,$8,now()+interval '5 minutes')")
        .bind(id).bind(p).bind(digest(&state)).bind(&nonce).bind(&input.challenge).bind(&verifier).bind(owner).bind(owner_session_hash)
        .execute(&app.0.pool).await?;
    let mut url =
        reqwest::Url::parse(&endpoint(&app, p, "authorize")).map_err(|_| Error::unavailable())?;
    {
        let mut q = url.query_pairs_mut();
        q.append_pair("client_id", id_client)
            .append_pair("redirect_uri", &callback_url(&app, p)?)
            .append_pair("response_type", "code")
            .append_pair("state", &state)
            .append_pair("nonce", &nonce);
        if p == "google" {
            q.append_pair("scope", "openid email profile")
                .append_pair("code_challenge_method", "S256")
                .append_pair(
                    "code_challenge",
                    &URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_ref().unwrap().as_bytes())),
                );
        } else {
            q.append_pair("scope", "name email")
                .append_pair("response_mode", "form_post");
        }
    }
    Ok(Json(
        json!({"authorization_url":url.to_string(),"attempt_id":id}),
    ))
}
#[derive(Deserialize)]
pub(crate) struct Callback {
    state: Option<String>,
    code: Option<String>,
    error: Option<String>,
}
pub(crate) async fn google_callback(
    State(app): State<Application>,
    Path(p): Path<String>,
    Query(input): Query<Callback>,
) -> Result<Response> {
    if p != "google" {
        return Err(Error::bad());
    }
    callback(app, p, input).await
}
pub(crate) async fn apple_callback(
    State(app): State<Application>,
    Path(p): Path<String>,
    Form(input): Form<Callback>,
) -> Result<Response> {
    if p != "apple" {
        return Err(Error::bad());
    }
    callback(app, p, input).await
}
async fn callback(app: Application, p: String, input: Callback) -> Result<Response> {
    let _permit = app
        .0
        .identity_requests
        .clone()
        .try_acquire_owned()
        .map_err(|_| Error(StatusCode::TOO_MANY_REQUESTS, "rate_limited"))?;
    auth::throttle(&app)?;
    let state = input.state.filter(|s| s.len() == 43).ok_or(Error::bad())?;
    let row=sqlx::query("UPDATE auth_attempts SET claimed=true WHERE state_hash=$1 AND provider=$2 AND expires_at>now() AND NOT claimed RETURNING id,nonce,code_verifier")
        .bind(digest(&state)).bind(&p).fetch_optional(&app.0.pool).await?.ok_or(Error::bad())?;
    let id: Uuid = row.get("id");
    let outcome = if input.error.is_some() {
        Err("provider_cancelled")
    } else if let Some(code) = input.code.filter(|s| !s.is_empty() && s.len() <= 4096) {
        redeem(&app, &p, &code, row.get("nonce"), row.get("code_verifier"))
            .await
            .map_err(|_| "provider_unavailable")
    } else {
        Err("provider_cancelled")
    };
    let opaque = random();
    let (subject, email, provider_name, picture_url, failure) = match outcome {
        Ok((subject, email, provider_name, picture_url)) => {
            (Some(subject), email, provider_name, picture_url, None)
        }
        Err(code) => (None, None, None, None, Some(code)),
    };
    sqlx::query("INSERT INTO auth_completions(code_hash,attempt_id,expires_at,provider,subject,email,provider_name,picture_url,failure) VALUES($1,$2,now()+interval '2 minutes',$3,$4,$5,$6,$7,$8)")
        .bind(digest(&opaque)).bind(id).bind(&p).bind(subject).bind(email).bind(provider_name).bind(picture_url).bind(failure).execute(&app.0.pool).await?;
    let scheme = app
        .0
        .config
        .identity
        .app_scheme
        .as_deref()
        .ok_or(Error::unavailable())?;
    let location = format!("{scheme}://auth/callback?code={opaque}");
    Ok((
        StatusCode::SEE_OTHER,
        [
            (header::LOCATION, location),
            (header::REFERRER_POLICY, "no-referrer".into()),
            (header::CACHE_CONTROL, "no-store".into()),
        ],
    )
        .into_response())
}
async fn bounded(response: reqwest::Response) -> anyhow::Result<Value> {
    anyhow::ensure!(response.status().is_success(), "provider response failed");
    anyhow::ensure!(
        response.content_length().is_none_or(|n| n <= 65536),
        "provider response too large"
    );
    let mut response = response;
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        anyhow::ensure!(
            bytes.len() + chunk.len() <= 65536,
            "provider response too large"
        );
        bytes.extend_from_slice(&chunk);
    }
    Ok(serde_json::from_slice(&bytes)?)
}
async fn keys(app: &Application, p: &str, force: bool) -> anyhow::Result<JwkSet> {
    if !force {
        let cache = app.0.jwks.lock().await;
        if let Some((at, keys)) = cache.get(p)
            && at.elapsed() < Duration::from_secs(3600)
        {
            return Ok(keys.clone());
        }
    }
    let response = app
        .0
        .client
        .get(endpoint(app, p, "keys"))
        .timeout(Duration::from_secs(10))
        .send()
        .await?;
    let set: JwkSet = serde_json::from_value(bounded(response).await?)?;
    anyhow::ensure!(!set.keys.is_empty() && set.keys.len() <= 32, "invalid keys");
    app.0
        .jwks
        .lock()
        .await
        .insert(p.into(), (Instant::now(), set.clone()));
    Ok(set)
}
async fn redeem(
    app: &Application,
    p: &str,
    code: &str,
    nonce: String,
    verifier: Option<String>,
) -> anyhow::Result<(String, Option<String>, Option<String>, Option<String>)> {
    let c = &app.0.config.identity;
    let client_id = if p == "google" {
        &c.google
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("google disabled"))?
            .client_id
    } else {
        &c.apple
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("apple disabled"))?
            .client_id
    };
    let secret = if p == "google" {
        c.google.as_ref().unwrap().client_secret.clone()
    } else {
        let apple = c.apple.as_ref().unwrap();
        let now = chrono::Utc::now().timestamp();
        let mut h = Header::new(Algorithm::ES256);
        h.kid = Some(apple.key_id.clone());
        encode(
            &h,
            &json!({"iss":apple.team_id,"sub":client_id,"aud":"https://appleid.apple.com","iat":now,"exp":now+300}),
            &EncodingKey::from_ec_pem(apple.private_key.as_bytes())?,
        )?
    };
    let mut form = vec![
        ("client_id", client_id.to_string()),
        ("client_secret", secret),
        ("code", code.to_string()),
        ("grant_type", "authorization_code".into()),
        (
            "redirect_uri",
            callback_url(app, p).map_err(|_| anyhow::anyhow!("missing callback"))?,
        ),
    ];
    if let Some(v) = verifier {
        form.push(("code_verifier", v));
    }
    let response = app
        .0
        .client
        .post(endpoint(app, p, "token"))
        .timeout(Duration::from_secs(10))
        .form(&form)
        .send()
        .await?;
    let token = bounded(response)
        .await?
        .get("id_token")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("missing id token"))?
        .to_owned();
    anyhow::ensure!(token.len() <= 16384, "oversized id token");
    let header = jsonwebtoken::decode_header(&token)?;
    anyhow::ensure!(header.alg == Algorithm::RS256, "unsupported signature");
    let kid = header.kid.ok_or_else(|| anyhow::anyhow!("missing kid"))?;
    let mut set = keys(app, p, false).await?;
    if !set
        .keys
        .iter()
        .any(|j| j.common.key_id.as_deref() == Some(&kid))
    {
        set = keys(app, p, true).await?;
    }
    let jwk = set
        .keys
        .iter()
        .find(|j| j.common.key_id.as_deref() == Some(&kid))
        .ok_or_else(|| anyhow::anyhow!("unknown kid"))?;
    let mut validation = Validation::new(Algorithm::RS256);
    validation.set_audience(&[client_id]);
    if p == "google" {
        validation.set_issuer(&["accounts.google.com", "https://accounts.google.com"]);
    } else {
        validation.set_issuer(&["https://appleid.apple.com"]);
    }
    validation.leeway = 0;
    let claims = decode::<Value>(&token, &DecodingKey::from_jwk(jwk)?, &validation)?.claims;
    anyhow::ensure!(
        claims.get("aud").and_then(Value::as_str) == Some(client_id),
        "audience mismatch"
    );
    anyhow::ensure!(
        claims.get("nonce").and_then(Value::as_str) == Some(&nonce),
        "nonce mismatch"
    );
    let sub = claims
        .get("sub")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("missing subject"))?;
    anyhow::ensure!(!sub.is_empty() && sub.len() <= 255, "invalid subject");
    let verified = matches!(claims.get("email_verified"), Some(Value::Bool(true)))
        || claims.get("email_verified").and_then(Value::as_str) == Some("true");
    let email = if verified {
        claims
            .get("email")
            .and_then(Value::as_str)
            .and_then(auth::email)
    } else {
        None
    };
    let provider_name = if p == "google" {
        claims
            .get("name")
            .and_then(Value::as_str)
            .and_then(|s| name(s).ok())
    } else {
        None
    };
    let picture_url = if p == "google" {
        claims
            .get("picture")
            .and_then(Value::as_str)
            .and_then(picture)
    } else {
        None
    };
    Ok((sub.into(), email, provider_name, picture_url))
}
// A conflict on the subject cannot move it to another account. Missing claims
// preserve previously verified attributes; local names are never touched.
async fn refresh_identity(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    p: &str,
    subject: &str,
    owner: Uuid,
    email: Option<&str>,
    provider_name: Option<&str>,
    picture_url: Option<&str>,
) -> Result<bool> {
    // Both callers hold the owner row lock. Refuse replacing this account's
    // existing provider subject before the unique constraint can abort the
    // transaction, so the valid native confirmation is still consumed.
    let linked_subject: Option<String> = sqlx::query_scalar(
        "SELECT subject FROM account_identities WHERE user_id=$1 AND provider=$2",
    )
    .bind(owner)
    .bind(p)
    .fetch_optional(&mut **tx)
    .await?;
    if linked_subject
        .as_deref()
        .is_some_and(|linked| linked != subject)
    {
        return Ok(false);
    }
    let email = if let Some(address) = email {
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
            .bind(address)
            .execute(&mut **tx)
            .await?;
        let claimed: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE email=$1 AND id<>$2 UNION ALL SELECT 1 FROM account_identities WHERE verified_email=$1 AND user_id<>$2)")
            .bind(address).bind(owner).fetch_one(&mut **tx).await?;
        if claimed { None } else { Some(address) }
    } else {
        None
    };
    let updated = sqlx::query("INSERT INTO account_identities(provider,subject,user_id,verified_email,provider_name,picture_url) VALUES($1,$2,$3,$4,$5,$6) ON CONFLICT (provider,subject) DO UPDATE SET verified_email=COALESCE(EXCLUDED.verified_email,account_identities.verified_email),provider_name=COALESCE(EXCLUDED.provider_name,account_identities.provider_name),picture_url=COALESCE(EXCLUDED.picture_url,account_identities.picture_url) WHERE account_identities.user_id=EXCLUDED.user_id")
        .bind(p).bind(subject).bind(owner).bind(email).bind(provider_name).bind(picture_url)
        .execute(&mut **tx).await?;
    if updated.rows_affected() == 0 {
        return Ok(false);
    }
    if let Some(name) = provider_name {
        sqlx::query("UPDATE users SET display_name=$1 WHERE id=$2 AND name_source=$3")
            .bind(name)
            .bind(owner)
            .bind(p)
            .execute(&mut **tx)
            .await?;
    }
    Ok(true)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Exchange {
    code: String,
    verifier: String,
}
pub(crate) async fn exchange(
    State(app): State<Application>,
    Json(input): Json<Exchange>,
) -> Result<Json<Value>> {
    auth::throttle(&app)?;
    if input.code.len() != 43 || input.verifier.len() < 43 || input.verifier.len() > 128 {
        return Err(Error::bad());
    }
    let mut tx = app.0.pool.begin().await?;
    let row=sqlx::query("SELECT c.attempt_id,c.provider,c.subject,c.email,c.provider_name,c.picture_url,c.failure,a.owner,a.owner_session_hash,a.challenge FROM auth_completions c JOIN auth_attempts a ON a.id=c.attempt_id WHERE c.code_hash=$1 AND c.expires_at>now() FOR UPDATE OF c")
        .bind(digest(&input.code)).fetch_optional(&mut *tx).await?.ok_or(Error::bad())?;
    if URL_SAFE_NO_PAD.encode(Sha256::digest(input.verifier.as_bytes()))
        != row.get::<String, _>("challenge")
    {
        return Err(Error::bad());
    }
    if let Some(owner) = row.get::<Option<Uuid>, _>("owner") {
        let session: Option<Uuid> = sqlx::query_scalar("SELECT user_id FROM sessions WHERE token_hash=$1 AND user_id=$2 AND expires_at>now() FOR UPDATE")
            .bind(row.get::<Option<Vec<u8>>, _>("owner_session_hash"))
            .bind(owner).fetch_optional(&mut *tx).await?;
        if session.is_none() {
            return Err(Error::bad());
        }
    }
    let attempt: Uuid = row.get("attempt_id");
    sqlx::query("DELETE FROM auth_attempts WHERE id=$1")
        .bind(attempt)
        .execute(&mut *tx)
        .await?;
    if let Some(failure) = row.get::<Option<String>, _>("failure") {
        tx.commit().await?;
        return Err(Error(
            StatusCode::BAD_REQUEST,
            if failure == "provider_cancelled" {
                "provider_cancelled"
            } else {
                "provider_unavailable"
            },
        ));
    }
    let p: String = row.get("provider");
    let subject: String = row.get("subject");
    let email: Option<String> = row.get("email");
    let provider_name: Option<String> = row.get("provider_name");
    let picture_url: Option<String> = row.get("picture_url");
    let owner: Option<Uuid> = row.get("owner");
    let existing: Option<Uuid> = sqlx::query_scalar(
        "SELECT user_id FROM account_identities WHERE provider=$1 AND subject=$2",
    )
    .bind(&p)
    .bind(&subject)
    .fetch_optional(&mut *tx)
    .await?;
    let result = if let Some(owner) = owner {
        if existing.is_some_and(|id| id != owner) {
            Err(Error(StatusCode::CONFLICT, "identity_already_linked"))
        } else {
            sqlx::query("SELECT id FROM users WHERE id=$1 FOR UPDATE")
                .bind(owner)
                .execute(&mut *tx)
                .await?;
            let linked = refresh_identity(
                &mut tx,
                &p,
                &subject,
                owner,
                email.as_deref(),
                provider_name.as_deref(),
                picture_url.as_deref(),
            )
            .await?;
            if !linked {
                Err(Error(StatusCode::CONFLICT, "identity_already_linked"))
            } else {
                let mut v = account_value(&mut tx, owner).await?;
                v["linked"] = json!(true);
                Ok(v)
            }
        }
    } else if let Some(user) = existing {
        // Lock the account before refreshing attributes or issuing a session.
        sqlx::query("SELECT id FROM users WHERE id=$1 FOR UPDATE")
            .bind(user)
            .execute(&mut *tx)
            .await?;
        let refreshed = refresh_identity(
            &mut tx,
            &p,
            &subject,
            user,
            email.as_deref(),
            provider_name.as_deref(),
            picture_url.as_deref(),
        )
        .await?;
        if refreshed {
            Ok(auth::issue(&mut tx, user).await?)
        } else {
            Err(Error(StatusCode::CONFLICT, "identity_already_linked"))
        }
    } else if let Some(email) = email {
        if !reserve_email(&mut tx, &email).await? {
            Err(Error(StatusCode::CONFLICT, "account_link_required"))
        } else {
            let id = Uuid::new_v4();
            let fallback: String = email
                .split('@')
                .next()
                .unwrap_or("User")
                .chars()
                .take(80)
                .collect();
            let display_name = provider_name.as_deref().unwrap_or(&fallback);
            let inserted=sqlx::query("INSERT INTO users(id,email,password_hash,display_name,name_source) VALUES($1,$2,NULL,$3,$4) ON CONFLICT DO NOTHING")
                .bind(id).bind(&email).bind(display_name).bind(&p).execute(&mut *tx).await?;
            if inserted.rows_affected() == 0 {
                Err(Error(StatusCode::CONFLICT, "account_link_required"))
            } else {
                let inserted=sqlx::query("INSERT INTO account_identities(provider,subject,user_id,verified_email,provider_name,picture_url) VALUES($1,$2,$3,$4,$5,$6) ON CONFLICT DO NOTHING")
                    .bind(&p).bind(&subject).bind(id).bind(&email).bind(&provider_name).bind(&picture_url).execute(&mut *tx).await?;
                if inserted.rows_affected() == 0 {
                    sqlx::query("DELETE FROM users WHERE id=$1")
                        .bind(id)
                        .execute(&mut *tx)
                        .await?;
                    Err(Error(StatusCode::CONFLICT, "identity_already_linked"))
                } else {
                    Ok(auth::issue(&mut tx, id).await?)
                }
            }
        }
    } else {
        Err(Error(StatusCode::CONFLICT, "account_link_required"))
    };
    // A valid proof is consumed even if account collision prevents authentication.
    tx.commit().await?;
    result.map(Json)
}
