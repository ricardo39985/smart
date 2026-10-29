//! Public account and proof-bound identity contracts against real PostgreSQL 18.
use proptest::strategy::{Strategy, ValueTree};
use serde_json::{Value, json};
use sha2::Digest;
use smartcindy::{Application, Config, GoogleIdentity};
use std::time::Duration;
use testcontainers::{
    GenericImage, ImageExt,
    core::{IntoContainerPort, WaitFor},
    runners::AsyncRunner,
};

#[tokio::test]
async fn local_signup_profile_login_logout_and_disabled_providers() {
    let db = GenericImage::new(
        "pgvector/pgvector",
        "pg18@sha256:ad249f9fb9572979643a47694e59b9a8cdcdf6abc6accebf49bb80ad5f20ff9b",
    )
    .with_exposed_port(5432.tcp())
    .with_wait_for(WaitFor::message_on_stderr(
        "listening on IPv4 address \"0.0.0.0\", port 5432",
    ))
    .with_env_var("POSTGRES_PASSWORD", "synthetic-password")
    .start()
    .await
    .unwrap();
    let url = format!(
        "postgres://postgres:synthetic-password@127.0.0.1:{}/postgres",
        db.get_host_port_ipv4(5432.tcp()).await.unwrap()
    );
    Application::migrate(&url).await.unwrap();
    let pool = sqlx::PgPool::connect(&url).await.unwrap();
    let provider_pool = pool.clone();
    let provider_url = url.clone();
    let app = Application::new(pool, Config::new(url)).await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, app.router()).await.unwrap() });
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    let r = client
        .get(format!("{base}/v1/auth/methods"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    assert_eq!(
        r.json::<Value>().await.unwrap(),
        json!({"password":true,"google":false,"apple":false})
    );
    let signup = json!({"email":" User@Example.COM ","password":"correct-password-123","display_name":"  First User  "});
    let r = client
        .post(format!("{base}/v1/accounts"))
        .json(&signup)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 201);
    let signed: Value = r.json().await.unwrap();
    let token = signed["token"].as_str().unwrap();
    assert_eq!(token.len(), 43);
    let r = client
        .get(format!("{base}/v1/account"))
        .bearer_auth(token)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let a: Value = r.json().await.unwrap();
    assert_eq!(a["id"], signed["user_id"]);
    assert_eq!(a["email"], "user@example.com");
    assert_eq!(a["display_name"], "First User");
    assert_eq!(a["methods"], json!(["password"]));
    let r = client
        .patch(format!("{base}/v1/account"))
        .bearer_auth(token)
        .json(&json!({"display_name":" Renamed "}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    assert_eq!(r.json::<Value>().await.unwrap()["display_name"], "Renamed");
    let r = client
        .post(format!("{base}/v1/accounts"))
        .json(&signup)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 409);
    assert_eq!(
        r.json::<Value>().await.unwrap()["error"]["code"],
        "account_exists"
    );
    let r = client
        .post(format!("{base}/v1/sessions"))
        .json(&json!({"email":"user@example.com","password":"incorrect-password"}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 401);
    let r = client
        .post(format!("{base}/v1/sessions"))
        .json(&json!({"email":"absent@example.com","password":"correct-password-123"}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 401);
    let r = client
        .post(format!("{base}/v1/sessions"))
        .json(&json!({"email":"user@example.com","password":"correct-password-123"}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let second: Value = r.json().await.unwrap();
    assert_eq!(second["user_id"], signed["user_id"]);
    let r = client
        .post(format!("{base}/v1/auth/google/start"))
        .json(&json!({"challenge":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 503);
    assert_eq!(
        r.json::<Value>().await.unwrap()["error"]["code"],
        "provider_unavailable"
    );
    // Public proof rejection property: no arbitrary 43-character completion authenticates.
    let mut runner = proptest::test_runner::TestRunner::new(proptest::test_runner::Config {
        cases: 8,
        rng_seed: proptest::test_runner::RngSeed::Fixed(73641982),
        ..Default::default()
    });
    let codes = proptest::string::string_regex("[A-Za-z0-9_-]{43}").unwrap();
    for _ in 0..8 {
        let code = codes.new_tree(&mut runner).unwrap().current();
        let r = client
            .post(format!("{base}/v1/auth/exchange"))
            .json(&json!({"code":code,"verifier":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"}))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 400);
    }
    let r = client
        .delete(format!("{base}/v1/sessions/current"))
        .bearer_auth(token)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 204);
    assert_eq!(
        client
            .get(format!("{base}/v1/account"))
            .bearer_auth(token)
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    assert_eq!(
        client
            .get(format!("{base}/v1/account"))
            .bearer_auth(second["token"].as_str().unwrap())
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    server.abort();

    // Explicit library-only loopback configuration: no provider network is used in this
    // cancellation journey. The browser result still requires the native S256 proof.
    let mut config = Config::new(provider_url);
    config.identity.google = Some(GoogleIdentity {
        client_id: "synthetic-client".into(),
        client_secret: "synthetic-secret".into(),
    });
    config.identity.origin = Some("https://dev.smartcindy.app".into());
    config.identity.app_scheme = Some("smartcindy-dev".into());
    config.identity.local_simulator = Some("http://127.0.0.1:59999/".into());
    let expiry_pool = provider_pool.clone();
    let app = Application::new(provider_pool, config).await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, app.router()).await.unwrap() });
    let browser = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let verifier = "native-verifier-with-adequate-length-12345678901234567890";
    let challenge = base64::Engine::encode(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD,
        sha2::Sha256::digest(verifier.as_bytes()),
    );
    let body = json!({"challenge":challenge,"link":true});
    assert_eq!(
        client
            .post(format!("{base}/v1/auth/google/start"))
            .json(&body)
            .send()
            .await
            .unwrap()
            .status(),
        400
    );
    let body =
        json!({"challenge":challenge,"link":true,"confirm":true,"password":"correct-password-123"});
    assert_eq!(
        client
            .post(format!("{base}/v1/auth/google/start"))
            .json(&body)
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    let wrong =
        json!({"challenge":challenge,"link":true,"confirm":true,"password":"incorrect-password"});
    assert_eq!(
        client
            .post(format!("{base}/v1/auth/google/start"))
            .bearer_auth(second["token"].as_str().unwrap())
            .json(&wrong)
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    let start = client
        .post(format!("{base}/v1/auth/google/start"))
        .bearer_auth(second["token"].as_str().unwrap())
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(start.status(), 200);
    let start: Value = start.json().await.unwrap();
    let url = reqwest::Url::parse(start["authorization_url"].as_str().unwrap()).unwrap();
    let state = url
        .query_pairs()
        .find(|(k, _)| k == "state")
        .unwrap()
        .1
        .into_owned();
    let bad = browser
        .get(format!(
            "{base}/v1/auth/apple/callback?state={state}&error=access_denied"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(bad.status(), 400);
    let bad = browser
        .get(format!(
            "{base}/v1/auth/google/callback?state=bogus&error=access_denied"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(bad.status(), 400);
    let callback = browser
        .get(format!(
            "{base}/v1/auth/google/callback?state={state}&error=access_denied"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(callback.status(), 303);
    let target = callback.headers()["location"].to_str().unwrap();
    assert!(target.starts_with("smartcindy-dev://auth/callback?code="));
    let code = target.split("code=").nth(1).unwrap();
    assert_eq!(
        browser
            .get(format!(
                "{base}/v1/auth/google/callback?state={state}&error=access_denied"
            ))
            .send()
            .await
            .unwrap()
            .status(),
        400
    );
    let bad=client.post(format!("{base}/v1/auth/exchange")).json(&json!({"code":code,"verifier":"wrong-verifier-with-adequate-length-12345678901234567890"})).send().await.unwrap();
    assert_eq!(bad.status(), 400);
    let result = client
        .post(format!("{base}/v1/auth/exchange"))
        .json(&json!({"code":code,"verifier":verifier}))
        .send()
        .await
        .unwrap();
    assert_eq!(result.status(), 400);
    assert_eq!(
        result.json::<Value>().await.unwrap()["error"]["code"],
        "provider_cancelled"
    );
    assert_eq!(
        client
            .post(format!("{base}/v1/auth/exchange"))
            .json(&json!({"code":code,"verifier":verifier}))
            .send()
            .await
            .unwrap()
            .status(),
        400
    );
    let profile = client
        .get(format!("{base}/v1/account"))
        .bearer_auth(second["token"].as_str().unwrap())
        .send()
        .await
        .unwrap()
        .json::<Value>()
        .await
        .unwrap();
    assert_eq!(profile["methods"], json!(["password"]));
    let expired = client
        .post(format!("{base}/v1/auth/google/start"))
        .json(&json!({"challenge":challenge}))
        .send()
        .await
        .unwrap()
        .json::<Value>()
        .await
        .unwrap();
    let expired_url = reqwest::Url::parse(expired["authorization_url"].as_str().unwrap()).unwrap();
    let expired_state = expired_url
        .query_pairs()
        .find(|(k, _)| k == "state")
        .unwrap()
        .1
        .into_owned();
    // Adjust only the database clock-bound expiry; rejection remains asserted via HTTP.
    sqlx::query("UPDATE auth_attempts SET expires_at=now()-interval '1 second' WHERE id=$1")
        .bind(uuid::Uuid::parse_str(expired["attempt_id"].as_str().unwrap()).unwrap())
        .execute(&expiry_pool)
        .await
        .unwrap();
    assert_eq!(
        browser
            .get(format!(
                "{base}/v1/auth/google/callback?state={expired_state}&error=access_denied"
            ))
            .send()
            .await
            .unwrap()
            .status(),
        400
    );
    server.abort();
}
