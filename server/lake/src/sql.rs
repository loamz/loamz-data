//! One database layer for `SQLite` and `Postgres`. The SQL is written once, in the subset both accept (TEXT ids, BIGINT
//! numbers, `$1` placeholders), so there is a single implementation to keep correct instead of two.

use std::sync::Once;

use sqlx::{
    AnyPool,
    any::{AnyPoolOptions, install_default_drivers},
    migrate::Migrator,
};

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("database: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("migration: {0}")]
    Migrate(#[from] sqlx::migrate::MigrateError),
    #[error("unsupported database url (use sqlite: or postgres:)")]
    Unsupported,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dialect {
    Sqlite,
    Postgres,
}

#[derive(Clone)]
pub struct Db {
    pool: AnyPool,
    dialect: Dialect,
}

static DRIVERS: Once = Once::new();

impl Db {
    /// Connects to `sqlite:path?mode=rwc` or `postgres://...`.
    pub async fn connect(url: &str) -> Result<Self, DbError> {
        DRIVERS.call_once(install_default_drivers);
        let dialect = if url.starts_with("sqlite:") {
            Dialect::Sqlite
        } else if url.starts_with("postgres:") || url.starts_with("postgresql:") {
            Dialect::Postgres
        } else {
            return Err(DbError::Unsupported);
        };
        // SQLite allows one writer at a time; a single connection avoids "database is locked" and keeps edge use simple.
        let max = if dialect == Dialect::Sqlite { 1 } else { 8 };
        let pool = AnyPoolOptions::new()
            .max_connections(max)
            .after_connect(move |conn, _| {
                Box::pin(async move {
                    if dialect == Dialect::Sqlite {
                        sqlx::query("PRAGMA journal_mode=WAL")
                            .execute(&mut *conn)
                            .await?;
                        sqlx::query("PRAGMA foreign_keys=ON")
                            .execute(&mut *conn)
                            .await?;
                        sqlx::query("PRAGMA busy_timeout=5000")
                            .execute(&mut *conn)
                            .await?;
                    }
                    Ok(())
                })
            })
            .connect(url)
            .await?;
        Ok(Self { pool, dialect })
    }

    #[must_use]
    pub fn pool(&self) -> &AnyPool {
        &self.pool
    }

    #[must_use]
    pub fn dialect(&self) -> Dialect {
        self.dialect
    }

    pub async fn migrate(&self, migrator: &Migrator) -> Result<(), DbError> {
        migrator.run(&self.pool).await?;
        Ok(())
    }
}
