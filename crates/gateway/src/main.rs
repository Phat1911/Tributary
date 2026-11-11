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
                 \x20             TRIBUTARY_MODELS, TRIBUTARY_LOG_FORMAT,\n\
                 \x20             TRIBUTARY_TRUSTED_PROXIES=<cidr,...> — XFF honored\n\
                 \x20             only from these proxies; unset ⇒ socket addr])\n\
                 migrate       run pending migrations, then exit\n\
                 create-admin  bootstrap the FIRST host admin (only when no users\n\
                 \x20             exist); further users are created via the admin API\n\
                 \x20             (options: --email <addr>; password via the\n\
                 \x20             TRIBUTARY_ADMIN_PASSWORD env var — preferred — or\n\
                 \x20             --password <secret>, which lands in shell history)"
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
    // A-019: `with_connect_info` stamps each request with the REAL
    // transport socket address (ConnectInfo) — the sanitize middleware's
    // trust decision needs the actual peer, not a header claim.
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
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

/// Env var carrying the bootstrap admin password (preferred over argv —
/// see [`resolve_admin_password`]).
const ENV_ADMIN_PASSWORD: &str = "TRIBUTARY_ADMIN_PASSWORD";

/// Resolves the first-admin password: `--password` argv wins, else the
/// `TRIBUTARY_ADMIN_PASSWORD` env var. One of the two must be present.
///
/// The env var is the DOCUMENTED path (docs prefer it) because an argv
/// password lands in the shell's history file and `docker exec` invocations
/// get copy-pasted into runbooks; the flag stays for scripts that build the
/// argument list in memory (v0.1.4 truth-release hardening).
fn resolve_admin_password(
    flag_password: Option<&str>,
    env_password: Option<&str>,
) -> anyhow::Result<String> {
    if let Some(p) = flag_password {
        return Ok(p.to_string());
    }
    // An EMPTY env var counts as unset — `TRIBUTARY_ADMIN_PASSWORD=` left
    // over in an env file must not silently become an empty password.
    if let Some(p) = env_password.filter(|p| !p.is_empty()) {
        return Ok(p);
    }
    anyhow::bail!(
        "create-admin requires a password: set {ENV_ADMIN_PASSWORD} (preferred) \
         or pass --password <secret> — argv passwords land in shell history"
    )
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
    let Some(email) = email else {
        anyhow::bail!("create-admin requires --email");
    };
    // `std::env::var` returns Result; `.ok()` flattens "unset" and "not valid
    // Unicode" into None — an undecodable env value behaves like unset and
    // the error below names both accepted paths.
    let env_password = std::env::var(ENV_ADMIN_PASSWORD).ok();
    let password = resolve_admin_password(password.as_deref(), env_password.as_deref())?;

    let url = Config::database_url_from_env()?;
    let db = db::Db::connect(&url).await?;
    db::run_migrations(&db).await?;
    let user = control::create_host_admin(&db, &email, &password).await?;
    println!("host admin created: {}", user.email);
    Ok(())
}

#[cfg(test)] // compile this module only for `cargo test` — unit tests below
mod tests {
    use super::resolve_admin_password;

    #[test]
    fn argv_password_wins_over_env() {
        let got = resolve_admin_password(Some("argv-secret"), Some("env-secret"))
            .expect("argv password must be accepted");
        assert_eq!(got, "argv-secret");
    }

    #[test]
    fn env_password_used_when_flag_absent() {
        let got = resolve_admin_password(None, Some("env-secret"))
            .expect("env password must be accepted");
        assert_eq!(got, "env-secret");
    }

    #[test]
    fn empty_env_counts_as_unset_and_both_absent_errors() {
        // Empty string env → treated as unset → falls through to the error.
        let err = resolve_admin_password(None, Some("")).expect_err("empty env must not pass");
        assert!(err.to_string().contains("TRIBUTARY_ADMIN_PASSWORD"));
        let err = resolve_admin_password(None, None).expect_err("no password must error");
        // The error must name BOTH accepted paths (operators read this).
        assert!(err.to_string().contains("TRIBUTARY_ADMIN_PASSWORD"));
        assert!(err.to_string().contains("--password"));
    }
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
