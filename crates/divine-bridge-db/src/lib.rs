//! Database access layer for the Divine Bridge.
//!
//! Provides Diesel models matching the 6 bridge tables and named queries
//! for idempotency checks and lookups.

pub mod models;
pub mod queries;
pub mod schema;

pub use queries::*;
