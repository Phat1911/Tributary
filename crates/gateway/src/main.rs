//! `tributary-gateway` binary: `serve`, `migrate`, and `create-admin`.
//!
//! Hand-rolled arg parsing on purpose — three fixed subcommands do not
//! justify a CLI-framework dependency (supply-chain discipline, workflow
//! step 10). M2: `seed-key` is gone (pool keys are issued per-pool by the
//! control API); `create-admin` bootstraps the first host admin.

use tributary_control as control;
use tributary_gateway::config::Config;
use tributary_gateway::{build_full_app, db};

#[tokio::main] // tokio: turns main into an async fn on the tokio runtime
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("serve") => serve().await,
        Some("migrate") => migrate().await,
        Some("create-admin") => create_admin(&args[1..]).await,
        _ => {
            eprintln!(
                "usage: tributary-gateway <serve|migrate|create-admin>\n\
                 \n\
                 serve         run gateway + control API (env: TRIBUTARY_DATABASE_URL,\n\
                 \x20             TRIBUTARY_MASTER_KEY [64-hex], [TRIBUTARY_BIND,\n\
                 \x20             TRIBUTARY_MODELS, TRIBUTARY_LOG_FORMAT])\n\
                 migrate       run pending migrations, then exit\n\
                 create-admin  bootstrap the FIRST host admin (only when no users\n\
                 \x20             exist); further users are created via the admin API\n\
                 \x20             (options: --email <addr> --password <secret>)"
            );
            std::process::exit(2);
        }
    }
}

/// Runs the single binary: data plane (`/v1/*`) + control plane (`/api/v1/*`).
async fn serve() -> anyhow::Result<()> {
    let config = Config::from_env()?;
    init_tracing(config.log_json);
    let bind = config.bind.clone();
    tracing::info!(bind = %bind, "tributary gateway starting");

    let db = db::Db::connect(&config.database_url).await?;
    let app = build_full_app(db, config).await?;
    let listener = tokio::net::TcpListener::bind(&bind).await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

/// Applies pending migrations and exits.
async fn migrate() -> anyhow::Result<()> {
    let url = Config::database_url_from_env()?;
    let db = db::Db::connect(&url).await?;
    db::run_migrations(&db).await?;
    println!("migrations applied");
    Ok(())
}

/// Bootstraps the first host admin (refuses when users already exist).
async fn create_admin(args: &[String]) -> anyhow::Result<()> {
    let mut email: Option<String> = None;
    let mut password: Option<String> = None;
    let mut iter = args.iter();
    while let Some(flag) = iter.next() {
        let value = iter
            .next()
            .ok_or_else(|| anyhow::anyhow!("missing value for {flag}"))?;
        match flag.as_str() {
            "--email" => email = Some(value.clone()),
            "--password" => password = Some(value.clone()),
            _ => anyhow::bail!("unknown create-admin option: {flag} {value}"),
        }
    }
    let (Some(email), Some(password)) = (email, password) else {
        anyhow::bail!("create-admin requires --email and --password");
    };

    let url = Config::database_url_from_env()?;
    let db = db::Db::connect(&url).await?;
    db::run_migrations(&db).await?;
    let user = control::create_host_admin(&db, &email, &password).await?;
    println!("host admin created: {}", user.email);
    Ok(())
}

/// Resolves when the process receives Ctrl-C (graceful shutdown hook).
async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
    tracing::info!("shutdown signal received");
}

/// Initializes tracing with env filtering; optional JSON output (§13).
fn init_tracing(json: bool) {
    use tracing_subscriber::EnvFilter;
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let builder = tracing_subscriber::fmt().with_env_filter(filter);
    if json {
        builder.json().init();
    } else {
        builder.init();
    }
}
