//! `loamz-data serve` runs the server; `loamz-data migrate` only brings the database up to date.

use std::process::ExitCode;

use loamz_data_server::{
    MIGRATIONS, compute,
    governance::lifecycle::service as lifecycle,
    kernel::{settings::Settings, state::App},
    router, shared,
};
use loamz_lake::sql::Db;

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    let command = std::env::args().nth(1).unwrap_or_default();
    if command != "serve" && command != "migrate" {
        eprintln!("usage: loamz-data serve | migrate");
        return ExitCode::from(2);
    }
    match run(&command).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("loamz-data: {e}");
            ExitCode::FAILURE
        }
    }
}

async fn run(command: &str) -> Result<(), String> {
    let settings = Settings::from_env()?;
    let db = Db::connect(&settings.database_url)
        .await
        .map_err(|e| e.to_string())?;
    db.migrate(&MIGRATIONS).await.map_err(|e| e.to_string())?;
    if command == "migrate" {
        tracing::info!("database is up to date");
        return Ok(());
    }
    let vault = settings.vault()?;
    let secrets = settings.resolve(vault.as_deref()).await?;
    let mut app = App::new(&settings, &secrets, db)?;
    app.vault = vault;
    loamz_data_server::storage::locations::service::attach(&app);
    let app = shared(app);
    app.verifier
        .refresh()
        .await
        .map_err(|e| format!("could not load the signing keys: {e}"))?;
    tokio::spawn(lifecycle::work_forever(app.clone()));
    tokio::spawn(compute::service::work_forever(app.clone()));
    tokio::spawn(compute::serverless::serve_forever(app.clone()));
    // The BI connection (Arrow Flight SQL) is off unless an address is given.
    if let Ok(addr) = std::env::var("LOAMZ_DATA_FLIGHT_LISTEN") {
        let addr: std::net::SocketAddr = addr
            .parse()
            .map_err(|_| "LOAMZ_DATA_FLIGHT_LISTEN must be an address like 127.0.0.1:18041")?;
        let app = app.clone();
        tokio::spawn(async move {
            if let Err(e) = loamz_data_server::bi::flight::serve(app, addr).await {
                tracing::error!(error = e, "the BI connection stopped");
            }
        });
    }
    let (router, _) = router(app);
    let listener = tokio::net::TcpListener::bind(settings.listen)
        .await
        .map_err(|e| e.to_string())?;
    tracing::info!(listen = %settings.listen, "loamz-data is ready");
    axum::serve(
        listener,
        router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(async {
        let _ = tokio::signal::ctrl_c().await;
    })
    .await
    .map_err(|e| e.to_string())
}
