//! Startup schema migration for the bridge database.
//!
//! The bridge DB has no external migration tooling (no `db-migrate` Job, no
//! `diesel_migrations` harness) and historically was hand-applied with `psql`.
//! That left production able to boot a bridge binary whose queries reference
//! columns the live schema does not have (e.g. the `004_publish_job_scheduler`
//! columns), which silently breaks publishing.
//!
//! To make startup self-sufficient, the bridge-owned migrations are embedded in
//! the binary and applied in explicit order on boot. Every statement is written
//! to be **idempotent** (`CREATE TABLE IF NOT EXISTS`, `ADD COLUMN IF NOT
//! EXISTS`, `CREATE INDEX IF NOT EXISTS`, `ALTER COLUMN ... SET DEFAULT`), so
//! re-running against a fresh, partially-migrated, or fully-migrated database is
//! safe. Explicit ordering also sidesteps the duplicate `004_` directory prefix
//! (`004_provisioning_keys` vs `004_publish_job_scheduler`) without renaming the
//! on-disk files.
//!
//! This intentionally does NOT own the appview/labeler migrations (002, 003) —
//! those tables belong to other services. Only the tables the bridge reads and
//! writes are covered here.

use anyhow::{Context, Result};
use diesel::connection::{Connection, SimpleConnection};
use diesel::pg::PgConnection;

/// A single embedded migration: a stable name (for logging) and its idempotent SQL.
struct EmbeddedMigration {
    name: &'static str,
    up_sql: &'static str,
}

/// Bridge-owned migrations, in the order they must apply.
///
/// Order matters: `001` creates `account_links` / `publish_jobs`; `004_publish_job_scheduler`
/// alters them; `005` adjusts a default. `004_provisioning_keys` is independent but listed
/// before the scheduler alter to keep a deterministic, prefix-collision-free sequence.
const MIGRATIONS: &[EmbeddedMigration] = &[
    EmbeddedMigration {
        name: "001_bridge_tables",
        up_sql: include_str!("../../../migrations/001_bridge_tables/up.sql"),
    },
    EmbeddedMigration {
        name: "004_provisioning_keys",
        up_sql: include_str!("../../../migrations/004_provisioning_keys/up.sql"),
    },
    EmbeddedMigration {
        name: "004_publish_job_scheduler",
        up_sql: include_str!("../../../migrations/004_publish_job_scheduler/up.sql"),
    },
    EmbeddedMigration {
        name: "005_crosspost_default_true",
        up_sql: include_str!("../../../migrations/005_crosspost_default_true/up.sql"),
    },
    EmbeddedMigration {
        name: "006_account_pds_session",
        up_sql: include_str!("../../../migrations/006_account_pds_session/up.sql"),
    },
    EmbeddedMigration {
        name: "007_operator_actions",
        up_sql: include_str!("../../../migrations/007_operator_actions/up.sql"),
    },
];

/// Apply all bridge-owned migrations to `database_url` on startup.
///
/// Each migration's SQL runs inside its own transaction via `batch_execute`, so
/// a partial failure rolls back that migration rather than leaving the schema
/// half-applied. Because the SQL is idempotent, this is safe to run on every
/// boot regardless of current schema state.
pub fn run_pending_migrations(database_url: &str) -> Result<()> {
    let mut conn = PgConnection::establish(database_url)
        .with_context(|| "failed to connect to bridge database for migrations")?;
    run_pending_migrations_on(&mut conn)
}

/// Apply migrations on an existing connection (used by tests).
pub fn run_pending_migrations_on(conn: &mut PgConnection) -> Result<()> {
    for migration in MIGRATIONS {
        // `batch_execute` sends the whole script unprepared, so multi-statement
        // migration files run as one unit. Wrapping in a transaction means a
        // partial failure rolls back rather than leaving a half-applied schema.
        conn.transaction::<_, diesel::result::Error, _>(|conn| {
            conn.batch_execute(migration.up_sql)
        })
        .with_context(|| format!("failed to apply migration {}", migration.name))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use diesel::sql_types::Int8;
    use diesel::{QueryableByName, RunQueryDsl};
    use std::sync::{Mutex, OnceLock};

    fn test_db_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn test_database_url() -> String {
        std::env::var("TEST_DATABASE_URL")
            .unwrap_or_else(|_| "postgres://divine:divine_dev@[::1]:5432/divine_bridge".to_string())
    }

    #[derive(QueryableByName)]
    struct CountRow {
        #[diesel(sql_type = Int8)]
        count: i64,
    }

    fn column_exists(conn: &mut PgConnection, table: &str, column: &str) -> bool {
        let sql = format!(
            "SELECT COUNT(*) AS count FROM information_schema.columns \
             WHERE table_name = '{table}' AND column_name = '{column}'"
        );
        diesel::sql_query(sql)
            .get_result::<CountRow>(conn)
            .unwrap()
            .count
            > 0
    }

    fn table_exists(conn: &mut PgConnection, table: &str) -> bool {
        let sql = format!(
            "SELECT COUNT(*) AS count FROM information_schema.tables \
             WHERE table_schema = current_schema() AND table_name = '{table}'"
        );
        diesel::sql_query(sql)
            .get_result::<CountRow>(conn)
            .unwrap()
            .count
            > 0
    }

    #[test]
    fn migrations_create_operator_action_audit_table() {
        let _guard = test_db_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut conn = PgConnection::establish(&test_database_url())
            .expect("test database should be reachable");
        run_pending_migrations_on(&mut conn).expect("migrations should run");
        assert!(table_exists(&mut conn, "operator_actions"));
        assert!(column_exists(
            &mut conn,
            "operator_actions",
            "before_images"
        ));
        assert!(column_exists(
            &mut conn,
            "operator_actions",
            "confirmation_digest"
        ));
    }

    /// Running migrations against a fresh, a partial, and an already-migrated
    /// database must all succeed and converge on the full schema.
    #[test]
    fn migrations_are_idempotent_across_db_states() {
        let _guard = test_db_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut conn = PgConnection::establish(&test_database_url())
            .expect("test database should be reachable");

        // Start from a clean slate.
        conn.batch_execute(
            "DROP TABLE IF EXISTS operator_actions, record_mappings, moderation_actions, publish_jobs, \
             asset_manifest, ingest_offsets, provisioning_keys, account_links CASCADE",
        )
        .unwrap();

        // Fresh DB: full run creates everything, including the 004 scheduler columns.
        run_pending_migrations_on(&mut conn).expect("fresh migration run should succeed");
        assert!(column_exists(&mut conn, "publish_jobs", "nostr_pubkey"));
        assert!(column_exists(
            &mut conn,
            "account_links",
            "publish_backfill_state"
        ));

        // Already-migrated DB: a second run must be a no-op, not an error.
        run_pending_migrations_on(&mut conn).expect("re-running migrations should be idempotent");
        assert!(column_exists(&mut conn, "publish_jobs", "lease_expires_at"));

        // Partial DB (the real prod scenario): only the pre-scheduler tables exist,
        // missing the 004 columns. A migration run must add them without erroring on
        // the tables that already exist.
        conn.batch_execute(
            "DROP TABLE IF EXISTS operator_actions, record_mappings, moderation_actions, publish_jobs, \
             asset_manifest, ingest_offsets, provisioning_keys, account_links CASCADE",
        )
        .unwrap();
        conn.batch_execute(include_str!("../../../migrations/001_bridge_tables/up.sql"))
            .expect("seed partial schema (001 only)");
        assert!(!column_exists(&mut conn, "publish_jobs", "nostr_pubkey"));
        run_pending_migrations_on(&mut conn).expect("partial migration run should succeed");
        assert!(column_exists(&mut conn, "publish_jobs", "nostr_pubkey"));
        assert!(column_exists(
            &mut conn,
            "account_links",
            "publish_backfill_state"
        ));
    }
}
