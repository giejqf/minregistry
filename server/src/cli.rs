//! Command line: `serve`, `openapi`, `migrate`, `gc`.

use std::{io::IsTerminal, process::ExitCode, time::Duration};

use clap::{Parser, Subcommand};

use crate::{
    audit::{action, AuditEvent, AuditLog, Outcome},
    config::{CoreConfig, LogFormat, ServeConfig},
    db::Db,
    gc::{self, GcOptions},
    App,
};

#[derive(Parser)]
#[command(name = "minregistry", version, about = "MinRegistry: a self-hosted OCI container registry")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run the registry, management API and web UI.
    Serve,
    /// Print the management API's OpenAPI document (web/openapi.json).
    Openapi,
    /// Apply database migrations and exit.
    Migrate,
    /// Garbage-collect unreferenced blobs.
    Gc {
        /// Report what would be deleted without deleting anything.
        #[arg(long)]
        dry_run: bool,
        /// Also delete manifests no tag reaches.
        #[arg(long)]
        delete_untagged: bool,
        /// Keep content younger than this (default: MINREGISTRY_GC_MIN_AGE).
        #[arg(long, value_parser = humantime::parse_duration)]
        min_age: Option<Duration>,
    },
}

pub fn main() -> ExitCode {
    let cli = Cli::parse();
    if let Command::Openapi = cli.command {
        print!("{}", crate::openapi_json());
        return ExitCode::SUCCESS;
    }
    let runtime = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("error: cannot start the async runtime: {e}");
            return ExitCode::FAILURE;
        }
    };
    let result = runtime.block_on(async move {
        match cli.command {
            Command::Serve => serve().await,
            Command::Migrate => migrate().await,
            Command::Gc { dry_run, delete_untagged, min_age } => run_gc(dry_run, delete_untagged, min_age).await,
            Command::Openapi => Ok(()),
        }
    });
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            if let Some(cfg) = e.downcast_ref::<crate::config::ConfigError>() {
                eprint!("{cfg}");
                return ExitCode::from(2);
            }
            tracing::error!(error = %format!("{e:#}"), "fatal");
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn init_tracing(core: &CoreConfig) {
    let filter = tracing_subscriber::EnvFilter::new(&core.log);
    let json = match core.log_format {
        LogFormat::Json => true,
        LogFormat::Pretty => false,
        LogFormat::Auto => !std::io::stderr().is_terminal(),
    };
    let builder = tracing_subscriber::fmt().with_env_filter(filter).with_writer(std::io::stderr);
    let _ =
        if json { builder.json().with_current_span(true).with_span_list(false).try_init() } else { builder.try_init() };
}

async fn serve() -> anyhow::Result<()> {
    let cfg = ServeConfig::from_env()?;
    init_tracing(&cfg.core);
    tracing::info!(listen = %cfg.listen, public_url = %cfg.public_url, storage = ?cfg.core.storage, "starting MinRegistry {}", env!("CARGO_PKG_VERSION"));
    let listen = cfg.listen;
    let app = App::build(cfg).await?;
    app.spawn_background_tasks();
    let listener = tokio::net::TcpListener::bind(listen).await?;
    app.serve(listener, shutdown_signal()).await?;
    tracing::info!("stopped");
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let term = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut s) => {
                s.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    #[cfg(not(unix))]
    let term = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {},
        _ = term => {},
    }
    tracing::info!("shutting down");
}

async fn migrate() -> anyhow::Result<()> {
    let core = CoreConfig::from_env()?;
    init_tracing(&core);
    let db = Db::open(&core.db_path).await?;
    db.migrate().await?;
    tracing::info!(db = %core.db_path.display(), "migrations applied");
    Ok(())
}

async fn run_gc(dry_run: bool, delete_untagged: bool, min_age: Option<Duration>) -> anyhow::Result<()> {
    let core = CoreConfig::from_env()?;
    init_tracing(&core);
    crate::install_crypto_provider();
    let db = Db::open(&core.db_path).await?;
    db.migrate().await?;
    let storage = crate::storage::from_config(&core.storage).await?;
    let opts = GcOptions { dry_run, delete_untagged, min_age: min_age.unwrap_or(core.gc_min_age) };
    let audit = AuditLog::new(db.clone());
    let event = AuditEvent::new(action::GC_RUN).principal_name("system").detail("trigger", "cli");
    match gc::run(&db, storage.as_ref(), &opts).await {
        Ok(report) => {
            audit.record(event.detail("report", serde_json::to_value(&report)?)).await;
            println!("{}", serde_json::to_string_pretty(&report)?);
            Ok(())
        }
        Err(e) => {
            audit.record(event.outcome(Outcome::Error).detail("error", e.to_string())).await;
            Err(e)
        }
    }
}
