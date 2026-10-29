//! Durable native chat backend. PostgreSQL is the transcript and job authority.
mod attachments;
mod auth;
mod chat;
mod http;
mod provider;
mod worker;

use sqlx::{PgPool, postgres::PgPoolOptions};
use std::{sync::Arc, time::Duration};
use tokio::sync::{Semaphore, watch};

/// Server-owned paid provider configuration. Never accepted from HTTP clients.
#[derive(Clone, Default)]
pub struct ProviderConfig {
    pub deepseek_key: Option<String>,
    pub bfl_key: Option<String>,
    /// Explicit local protocol simulator, disabled in normal operation.
    pub local_simulator: Option<String>,
}
#[derive(Clone)]
pub struct Config {
    pub database_url: String,
    pub providers: ProviderConfig,
    /// At most two simultaneous paid generations per process.
    pub workers: usize,
    pub pool_connections: u32,
}
impl Config {
    pub fn new(database_url: String) -> Self {
        Self {
            database_url,
            providers: ProviderConfig::default(),
            workers: 2,
            pool_connections: 12,
        }
    }
}

#[derive(Clone)]
pub struct Application(pub(crate) Arc<State>);
pub(crate) struct State {
    pub pool: PgPool,
    pub config: Config,
    pub client: reqwest::Client,
    pub hashes: Arc<Semaphore>,
    pub images: Arc<Semaphore>,
    pub streams: Arc<Semaphore>,
    pub login_window: std::sync::Mutex<(std::time::Instant, u32)>,
    pub dummy_hash: String,
}
impl Application {
    /// Connect to the runtime pool; does not run migrations (safe for PgBouncer).
    pub async fn connect(config: Config) -> anyhow::Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(config.pool_connections)
            .acquire_timeout(Duration::from_secs(5))
            .connect(&config.database_url)
            .await?;
        Self::new(pool, config).await
    }
    pub async fn new(pool: PgPool, config: Config) -> anyhow::Result<Self> {
        anyhow::ensure!((1..=2).contains(&config.workers), "workers must be 1..=2");
        anyhow::ensure!(
            (4..=32).contains(&config.pool_connections),
            "pool must be 4..=32"
        );
        if let Some(base) = &config.providers.local_simulator {
            let url = reqwest::Url::parse(base)?;
            anyhow::ensure!(
                url.scheme() == "http"
                    && matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"))
                    && url.username().is_empty()
                    && url.password().is_none(),
                "simulator must be explicit loopback HTTP"
            );
        }
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(115))
            .build()?;
        let dummy_hash =
            tokio::task::spawn_blocking(|| auth::hash_password("nonexistent-account-dummy".into()))
                .await??;
        Ok(Self(Arc::new(State {
            pool,
            config,
            client,
            hashes: Arc::new(Semaphore::new(2)),
            images: Arc::new(Semaphore::new(2)),
            streams: Arc::new(Semaphore::new(32)),
            login_window: std::sync::Mutex::new((std::time::Instant::now(), 0)),
            dummy_hash,
        })))
    }
    /// Use a direct PostgreSQL URL, not the transaction-pooled runtime URL.
    pub async fn migrate(direct_url: &str) -> anyhow::Result<()> {
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(direct_url)
            .await?;
        let result = sqlx::migrate!("./migrations").run(&pool).await;
        pool.close().await;
        result?;
        Ok(())
    }
    pub fn router(&self) -> axum::Router {
        http::router(self.clone())
    }
    /// Operator-only account creation. Passwords must come from protected input, not argv.
    pub async fn provision_user(
        &self,
        email: &str,
        password: String,
    ) -> anyhow::Result<uuid::Uuid> {
        auth::provision(self, email, password).await
    }
    /// Starts bounded workers. Shutdown interrupts local futures; unknown paid outcomes fail closed.
    pub fn run_worker(&self) -> WorkerHandle {
        let (tx, rx) = watch::channel(false);
        let app = self.clone();
        let join = tokio::spawn(async move { worker::run(app, rx).await });
        WorkerHandle { tx, join }
    }
}

pub struct WorkerHandle {
    tx: watch::Sender<bool>,
    join: tokio::task::JoinHandle<anyhow::Result<()>>,
}
impl WorkerHandle {
    pub async fn shutdown(self) -> anyhow::Result<()> {
        let _ = self.tx.send(true);
        self.join.await?
    }
    /// Surfaces a worker database/task fault without swallowing it in a retry loop.
    pub async fn wait(&mut self) -> anyhow::Result<()> {
        (&mut self.join).await?
    }
}
