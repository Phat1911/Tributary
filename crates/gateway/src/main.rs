//! `tributary-gateway` binary: `serve`, `migrate`, and `seed-key`.
//!
//! Hand-rolled arg parsing on purpose — three fixed subcommands do not
//! justify a CLI-framework dependency (supply-chain discipline, workflow
//! step 10).

use tributary_gateway::config::Config;
use tributary_gateway::{build_app, build_state, db, keys};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("serve") => serve().await,
        Some("migrate") => migrate().await,
        Some("seed-key") => seed_key(&args[1..]).await,
        _ => {
            eprintln!(
                "usage: tributary-gateway <serve|migrate|seed-key>\n\
                 \n\
                 serve      run the gateway (env: TRIBUTARY_DATABASE_URL,\n\
                 \x20          TRIBUTARY_OPENAI_API_KEY, [TRIBUTARY_OPENAI_BASE_URL,\n\
                 \x20          TRIBUTARY_BIND, TRIBUTARY_MODELS, TRIBUTARY_LOG_FORMAT])\n\
                 migrate    run pending migrations, then exit\n\
                 seed-key   create one pool key; prints the raw key ONCE\n\
                 \x20          (options: --user <id> --expires <unix-ms> --kind <live|canary>)"
            );
            std::process::exit(2);
        }
    }
}

async fn serve() -> anyhow::Result<()> {
    let config = Config::from_env()?;
    init_tracing(config.log_json);
    let bind = config.bind.clone();
    tracing::info!(bind = %bind, models = ?config.models, "tributary gateway starting");

    let state = build_state(config).await?;
    let app = build_app(state);
    let listener = tokio::net::TcpListener::bind(&bind).await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

async fn migrate() -> anyhow::Result<()> {
    let url = Config::database_url_from_env()?;
    let db = db::Db::connect(&url).await?;
    db::run_migrations(&db).await?;
    println!("migrations applied");
    Ok(())
}

async fn seed_key(args: &[String]) -> anyhow::Result<()> {
    let mut user_id: Option<String> = None;
    let mut expires_at: Option<i64> = None;
    let mut kind = "live".to_string();

    let mut iter = args.iter();
    while let Some(flag) = iter.next() {
        let value = iter
            .next()
            .ok_or_else(|| anyhow::anyhow!("missing value for {flag}"))?;
        match flag.as_str() {
            "--user" => user_id = Some(value.clone()),
            "--expires" => expires_at = Some(value.parse::<i64>()?),
            "--kind" if value == "live" || value == "canary" => kind = value.clone(),
            _ => anyhow::bail!("unknown seed-key option: {flag} {value}"),
        }
    }

    let url = Config::database_url_from_env()?;
    let db = db::Db::connect(&url).await?;
    db::run_migrations(&db).await?;

    let key = keys::generate_key();
    db::insert_pool_key(
        &db,
        &db::NewPoolKey {
            id: db::new_id(),
            key_hash: key.key_hash,
            key_prefix: key.key_prefix.clone(),
            kind,
            status: "active".to_string(),
            expires_at,
            user_id,
            created_at: db::now_ms(),
        },
    )
    .await?;

    // One-time reveal of the raw key on stdout only (never via tracing).
    println!("pool key created — store it now, it will not be shown again:");
    println!("  raw:    {}", key.raw);
    println!("  display: {}", key.key_prefix);
    Ok(())
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
    tracing::info!("shutdown signal received");
}

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
