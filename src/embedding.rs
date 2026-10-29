//! Private, bounded local inference. No paid embedding provider or fallback submission.
use crate::Application;
use futures_util::StreamExt;
use serde::Deserialize;
use std::{sync::LazyLock, time::Duration};
use tokio::sync::Semaphore;

pub(crate) const MODEL_REVISION: &str = "c721113d59a1d91b447450324f51c4b3332c924a";
static IN_FLIGHT: Semaphore = Semaphore::const_new(2);
// The private transport must never inherit a proxy that could forward user text off-host.
static CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_millis(250))
        .timeout(Duration::from_secs(2))
        .build()
        .expect("local HTTP client configuration")
});

pub(crate) fn validate_url(value: &Option<String>) -> anyhow::Result<()> {
    if let Some(value) = value {
        let url = reqwest::Url::parse(value)?;
        anyhow::ensure!(
            url.scheme() == "http"
                && matches!(url.host_str(), Some("127.0.0.1" | "[::1]"))
                && url.username().is_empty()
                && url.password().is_none()
                && url.query().is_none()
                && url.fragment().is_none()
                && url.path() == "/",
            "embedding origin must be literal loopback HTTP"
        );
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Response {
    model_revision: String,
    embeddings: Vec<Vec<f32>>,
}

pub(crate) async fn embed(app: &Application, texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>> {
    anyhow::ensure!(
        !texts.is_empty()
            && texts.len() <= 16
            && texts
                .iter()
                .all(|s| !s.trim().is_empty() && s.len() <= 4000),
        "embedding input limit"
    );
    let base = app
        .0
        .config
        .embedding_url
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("embedding disabled"))?;
    // Admission fails promptly instead of hiding an unbounded CPU queue on the reply path.
    let _permit = IN_FLIGHT
        .try_acquire()
        .map_err(|_| anyhow::anyhow!("embedding busy"))?;
    let response = CLIENT
        .post(format!("{}/embed", base.trim_end_matches('/')))
        .timeout(Duration::from_secs(2))
        .json(&serde_json::json!({"texts":texts}))
        .send()
        .await?;
    anyhow::ensure!(response.status().is_success(), "embedding unavailable");
    anyhow::ensure!(
        response.content_length().is_none_or(|n| n <= 131072),
        "embedding response limit"
    );
    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        anyhow::ensure!(
            bytes.len() + chunk.len() <= 131072,
            "embedding response limit"
        );
        bytes.extend_from_slice(&chunk);
    }
    let response: Response = serde_json::from_slice(&bytes)?;
    anyhow::ensure!(
        response.model_revision == MODEL_REVISION && response.embeddings.len() == texts.len(),
        "embedding identity mismatch"
    );
    for vector in &response.embeddings {
        let norm: f32 = vector.iter().map(|x| x * x).sum();
        anyhow::ensure!(
            vector.len() == 384
                && vector.iter().all(|x| x.is_finite())
                && (0.99..=1.01).contains(&norm),
            "invalid embedding"
        );
    }
    Ok(response.embeddings)
}
