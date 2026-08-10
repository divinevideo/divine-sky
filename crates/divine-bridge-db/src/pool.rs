//! Shared Postgres connection pool for all bridge services.
//!
//! Every service previously either opened a fresh `PgConnection` per operation
//! (connection churn, blocking establish inside async handlers) or shared a
//! single `Arc<Mutex<PgConnection>>` (serialized access, no reconnect on a
//! dropped connection, and a poisoned-mutex cascade if any query panicked while
//! holding the lock). A pool fixes all three: connections are reused, dead
//! connections are transparently replaced, and acquisition is concurrent.
//!
//! Blocking Diesel calls should still run off the async runtime — acquire a
//! connection with [`DbPool::get`] inside `tokio::task::spawn_blocking`.

use anyhow::{bail, Context, Result};
use diesel::pg::PgConnection;
use diesel::r2d2::{ConnectionManager, Pool, PooledConnection};
use std::env::VarError;

/// A pooled-connection handle to the bridge database.
pub type DbPool = Pool<ConnectionManager<PgConnection>>;

/// A connection checked out of a [`DbPool`]. Derefs to `PgConnection`, so it can
/// be passed to the query helpers that take `&mut PgConnection`.
pub type PooledConn = PooledConnection<ConnectionManager<PgConnection>>;

/// Default maximum number of pooled connections per service instance.
///
/// Overridable via `DB_POOL_MAX_SIZE`. Kept modest so several service replicas
/// stay well under Postgres `max_connections`.
const DEFAULT_MAX_SIZE: u32 = 10;

/// Build a connection pool for `database_url`.
///
/// The pool eagerly opens one connection to fail fast when the database is
/// unreachable at startup, rather than surfacing the error on the first request.
pub fn build_pool(database_url: &str) -> Result<DbPool> {
    let max_size = db_pool_max_size_from_env()?;

    let manager = ConnectionManager::<PgConnection>::new(database_url);
    Pool::builder()
        .max_size(max_size)
        .min_idle(Some(1))
        .build(manager)
        .context("failed to build database connection pool")
}

fn db_pool_max_size_from_env() -> Result<u32> {
    let raw = match std::env::var("DB_POOL_MAX_SIZE") {
        Ok(value) => value,
        Err(VarError::NotPresent) => return Ok(DEFAULT_MAX_SIZE),
        Err(error) => return Err(error).context("failed to read DB_POOL_MAX_SIZE"),
    };

    let max_size = raw
        .parse::<u32>()
        .with_context(|| format!("DB_POOL_MAX_SIZE must be a positive integer, got {raw:?}"))?;
    if max_size == 0 {
        bail!("DB_POOL_MAX_SIZE must be greater than 0");
    }
    Ok(max_size)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    // `build_pool` reads the process-global `DB_POOL_MAX_SIZE`; serialize the
    // tests that mutate it so parallel execution cannot observe a torn value.
    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    // CI (and local runs) export TEST_DATABASE_URL; require it rather than a
    // fallback so every line of this helper executes under coverage.
    fn test_database_url() -> String {
        std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL must be set for pool tests")
    }

    #[test]
    fn build_pool_uses_default_max_size_and_opens_a_connection() {
        let guard = env_lock().lock().unwrap();
        std::env::remove_var("DB_POOL_MAX_SIZE");
        let pool = build_pool(&test_database_url()).expect("pool builds against the test database");
        assert_eq!(pool.max_size(), DEFAULT_MAX_SIZE);
        let conn = pool.get().expect("a connection can be checked out");
        drop(conn);
        drop(guard);
    }

    #[test]
    fn build_pool_honors_db_pool_max_size_override() {
        let guard = env_lock().lock().unwrap();
        std::env::set_var("DB_POOL_MAX_SIZE", "3");
        let pool = build_pool(&test_database_url()).expect("pool builds with an overridden size");
        std::env::remove_var("DB_POOL_MAX_SIZE");
        assert_eq!(pool.max_size(), 3);
        drop(guard);
    }

    #[test]
    fn build_pool_errors_on_an_unparseable_db_pool_max_size() {
        let guard = env_lock().lock().unwrap();
        std::env::set_var("DB_POOL_MAX_SIZE", "not-a-number");
        let error =
            build_pool("postgres://unused").expect_err("bad pool size should fail before connect");
        std::env::remove_var("DB_POOL_MAX_SIZE");
        assert!(error.to_string().contains("DB_POOL_MAX_SIZE"));
        drop(guard);
    }

    #[test]
    fn build_pool_errors_on_zero_db_pool_max_size() {
        let guard = env_lock().lock().unwrap();
        std::env::set_var("DB_POOL_MAX_SIZE", "0");
        let error =
            build_pool("postgres://unused").expect_err("zero pool size should fail before connect");
        std::env::remove_var("DB_POOL_MAX_SIZE");
        assert!(error.to_string().contains("greater than 0"));
        drop(guard);
    }

    #[test]
    fn build_pool_errors_when_the_database_is_unreachable() {
        let guard = env_lock().lock().unwrap();
        std::env::remove_var("DB_POOL_MAX_SIZE");
        let error = build_pool("postgres://divine:divine_dev@127.0.0.1:1/divine_bridge")
            .expect_err("an unreachable database must fail the eager connection");
        assert!(error.to_string().contains("connection pool"));
        drop(guard);
    }
}
