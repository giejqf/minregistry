//! SQLite access. One file, WAL mode; writes go through a single-connection
//! pool, reads through a separate pool. Queries are grouped by table and use
//! the compile-time checked `sqlx::query!` macros (offline data in `.sqlx/`).

use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::Context;
use sqlx::{
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
    Sqlite, SqlitePool, Transaction,
};

pub(crate) mod audit;
pub(crate) mod blobs;
pub(crate) mod manifests;
pub(crate) mod permissions;
pub(crate) mod principals;
pub(crate) mod repositories;
pub(crate) mod sessions;
pub(crate) mod tags;
pub(crate) mod tokens;
pub(crate) mod uploads;

pub(crate) type Tx = Transaction<'static, Sqlite>;

#[derive(Clone)]
pub(crate) struct Db {
    pub(crate) read: SqlitePool,
    pub(crate) write: SqlitePool,
    path: PathBuf,
}

const PRAGMAS: &str =
    "PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON; PRAGMA busy_timeout=5000; PRAGMA synchronous=NORMAL;";

impl Db {
    pub(crate) async fn open(path: &Path) -> anyhow::Result<Self> {
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            tokio::fs::create_dir_all(dir).await.with_context(|| format!("creating {}", dir.display()))?;
        }
        let options =
            SqliteConnectOptions::new().filename(path).create_if_missing(true).busy_timeout(Duration::from_secs(5));
        let write = SqlitePoolOptions::new()
            .max_connections(1)
            .after_connect(|conn, _| Box::pin(async move { sqlx::raw_sql(PRAGMAS).execute(conn).await.map(|_| ()) }))
            .connect_with(options.clone())
            .await
            .with_context(|| format!("opening database {}", path.display()))?;
        let read = SqlitePoolOptions::new()
            .max_connections(8)
            .after_connect(|conn, _| Box::pin(async move { sqlx::raw_sql(PRAGMAS).execute(conn).await.map(|_| ()) }))
            .connect_with(options)
            .await
            .with_context(|| format!("opening database {}", path.display()))?;
        Ok(Db { read, write, path: path.to_path_buf() })
    }

    pub(crate) async fn migrate(&self) -> anyhow::Result<()> {
        sqlx::migrate!("./migrations").run(&self.write).await.context("applying database migrations")?;
        Ok(())
    }

    /// Starts a write transaction that holds SQLite's write lock from the
    /// first statement, so concurrent writers (including other processes
    /// such as `minregistry gc`) serialize instead of failing to upgrade.
    pub(crate) async fn begin_write(&self) -> sqlx::Result<Tx> {
        self.write.begin_with("BEGIN IMMEDIATE").await
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    /// Size of the database file plus its WAL, in bytes.
    pub(crate) async fn size_bytes(&self) -> u64 {
        let mut total = 0;
        for suffix in ["", "-wal"] {
            let mut p = self.path.clone().into_os_string();
            p.push(suffix);
            if let Ok(m) = tokio::fs::metadata(PathBuf::from(p)).await {
                total += m.len();
            }
        }
        total
    }

    pub(crate) async fn ping(&self) -> sqlx::Result<()> {
        sqlx::query("SELECT 1").execute(&self.read).await.map(|_| ())
    }
}

/// True when `e` is a UNIQUE / PRIMARY KEY constraint violation.
pub(crate) fn is_unique_violation(e: &sqlx::Error) -> bool {
    matches!(e, sqlx::Error::Database(db) if db.is_unique_violation())
}
