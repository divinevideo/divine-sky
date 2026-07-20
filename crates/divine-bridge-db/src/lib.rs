//! Database access layer for the Divine Bridge.
//!
//! Provides Diesel models matching the 6 bridge tables and named queries
//! for idempotency checks and lookups.

pub mod migrations;
pub mod models;
pub mod pool;
pub mod queries;
pub mod schema;

pub use migrations::run_pending_migrations;
pub use pool::{build_pool, DbPool, PooledConn};
pub use queries::*;
