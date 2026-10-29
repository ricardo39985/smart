use smartcindy::{AppleIdentity, Application, Config, GoogleIdentity, ProviderConfig};
use std::{future::IntoFuture, io::Read};

#[tokio::main]
async fn main() {
    if run().await.is_err() {
        // Never format transport/database errors: they can contain URLs, content or credentials.
        eprintln!("smartcindy operation failed; inspect configuration and database availability");
        std::process::exit(1);
    }
}
async fn run() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("migrate") {
        anyhow::ensure!(args.len() == 1, "usage: smartcindy migrate");
        return Application::migrate(&std::env::var("MIGRATION_DATABASE_URL")?).await;
    }
    let mut config = Config::new(std::env::var("DATABASE_URL")?);
    config.providers = ProviderConfig {
        deepseek_key: std::env::var("DEEPSEEK_API_KEY")
            .ok()
            .filter(|s| !s.is_empty()),
        bfl_key: std::env::var("BFL_API_KEY").ok().filter(|s| !s.is_empty()),
        local_simulator: None,
    };
    let google_id = std::env::var("GOOGLE_CLIENT_ID")
        .ok()
        .filter(|s| !s.is_empty());
    let google_secret = std::env::var("GOOGLE_CLIENT_SECRET")
        .ok()
        .filter(|s| !s.is_empty());
    anyhow::ensure!(
        google_id.is_some() == google_secret.is_some(),
        "incomplete google configuration"
    );
    config.identity.google = google_id
        .zip(google_secret)
        .map(|(client_id, client_secret)| GoogleIdentity {
            client_id,
            client_secret,
        });
    let apple_id = std::env::var("APPLE_CLIENT_ID")
        .ok()
        .filter(|s| !s.is_empty());
    let apple_team = std::env::var("APPLE_TEAM_ID")
        .ok()
        .filter(|s| !s.is_empty());
    let apple_key = std::env::var("APPLE_KEY_ID").ok().filter(|s| !s.is_empty());
    let apple_file = std::env::var("APPLE_PRIVATE_KEY_FILE")
        .ok()
        .filter(|s| !s.is_empty());
    let apple_fields = [
        apple_id.is_some(),
        apple_team.is_some(),
        apple_key.is_some(),
        apple_file.is_some(),
    ];
    anyhow::ensure!(
        apple_fields.iter().all(|&b| b) || apple_fields.iter().all(|&b| !b),
        "incomplete apple configuration"
    );
    if let (Some(client_id), Some(team_id), Some(key_id), Some(file)) =
        (apple_id, apple_team, apple_key, apple_file)
    {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            anyhow::ensure!(
                std::fs::metadata(&file)?.permissions().mode() & 0o077 == 0,
                "apple key file must not be accessible to group or others"
            );
        }
        config.identity.apple = Some(AppleIdentity {
            client_id,
            team_id,
            key_id,
            private_key: std::fs::read_to_string(file)?,
        });
    }
    config.identity.origin = std::env::var("AUTH_ORIGIN").ok().filter(|s| !s.is_empty());
    config.identity.app_scheme = std::env::var("AUTH_APP_SCHEME")
        .ok()
        .filter(|s| !s.is_empty());
    let app = Application::connect(config).await?;
    if args.first().map(String::as_str) == Some("provision-user") {
        anyhow::ensure!(
            args.len() == 2,
            "usage: smartcindy provision-user EMAIL (password on stdin)"
        );
        let password = match std::env::var("SMARTCINDY_PROVISION_PASSWORD") {
            Ok(password) => password,
            Err(_) => {
                let mut password = String::new();
                std::io::stdin().take(1026).read_to_string(&mut password)?;
                if password.ends_with('\n') {
                    password.pop();
                    if password.ends_with('\r') {
                        password.pop();
                    }
                }
                password
            }
        };
        app.provision_user(&args[1], password).await?;
        return Ok(());
    }
    anyhow::ensure!(
        args.is_empty() || args == ["serve"],
        "usage: smartcindy [serve|migrate|provision-user EMAIL]"
    );
    let address = std::env::var("LISTEN_ADDR").unwrap_or_else(|_| "127.0.0.1:8787".into());
    let listener = tokio::net::TcpListener::bind(address).await?;
    let mut worker = app.run_worker();
    // Worker faults stop serving instead of silently leaving a live API with dead workers.
    let (stop, rx) = tokio::sync::oneshot::channel();
    let server = axum::serve(listener, app.router())
        .with_graceful_shutdown(async {
            let _ = rx.await;
        })
        .into_future();
    tokio::pin!(server);
    tokio::select! {
        result=&mut server=>{worker.shutdown().await?;result?;},
        result=worker.wait()=>{let _=stop.send(());server.await?;result?;},
        _=tokio::signal::ctrl_c()=>{worker.shutdown().await?;let _=stop.send(());server.await?;},
    }
    // Process shutdown drops remaining local futures. Running leases are recovered as interrupted.
    Ok(())
}
