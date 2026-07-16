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

use anyhow::{Context, Result};
use diesel::pg::PgConnection;
use diesel::r2d2::{ConnectionManager, Pool, PooledConnection};

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
    let max_size = std::env::var("DB_POOL_MAX_SIZE")
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(DEFAULT_MAX_SIZE);

    let manager = ConnectionManager::<PgConnection>::new(database_url);
    Pool::builder()
        .max_size(max_size)
        .build(manager)
        .context("failed to build database connection pool")
}
