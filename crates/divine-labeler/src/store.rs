//! Database store wrapping divine-bridge-db queries for the labeler.

use anyhow::Result;

use divine_bridge_db::models::{LabelerEvent, NewLabelerEvent};
use divine_bridge_db::pool::{build_pool, DbPool};
use divine_bridge_db::queries;

#[derive(Debug)]
struct OwnedNewLabelerEvent {
    src_did: String,
    subject_uri: String,
    subject_cid: Option<String>,
    val: String,
    neg: bool,
    nostr_event_id: Option<String>,
    sha256: Option<String>,
    origin: String,
}

impl OwnedNewLabelerEvent {
    fn from_borrowed(event: &NewLabelerEvent<'_>) -> Self {
        Self {
            src_did: event.src_did.to_string(),
            subject_uri: event.subject_uri.to_string(),
            subject_cid: event.subject_cid.map(str::to_string),
            val: event.val.to_string(),
            neg: event.neg,
            nostr_event_id: event.nostr_event_id.map(str::to_string),
            sha256: event.sha256.map(str::to_string),
            origin: event.origin.to_string(),
        }
    }

    fn as_borrowed(&self) -> NewLabelerEvent<'_> {
        NewLabelerEvent {
            src_did: &self.src_did,
            subject_uri: &self.subject_uri,
            subject_cid: self.subject_cid.as_deref(),
            val: &self.val,
            neg: self.neg,
            nostr_event_id: self.nostr_event_id.as_deref(),
            sha256: self.sha256.as_deref(),
            origin: &self.origin,
        }
    }
}

#[derive(Clone)]
pub struct DbStore {
    pool: DbPool,
}

impl DbStore {
    pub fn connect(database_url: &str) -> Result<Self> {
        Ok(Self {
            pool: build_pool(database_url)?,
        })
    }

    pub async fn insert_labeler_event(&self, event: &NewLabelerEvent<'_>) -> Result<LabelerEvent> {
        let pool = self.pool.clone();
        let event = OwnedNewLabelerEvent::from_borrowed(event);
        tokio::task::spawn_blocking(move || {
            let mut conn = pool.get()?;
            queries::insert_labeler_event(&mut conn, &event.as_borrowed())
        })
        .await?
    }

    pub async fn get_events_after(&self, after_seq: i64, limit: i64) -> Result<Vec<LabelerEvent>> {
        let pool = self.pool.clone();
        tokio::task::spawn_blocking(move || {
            let mut conn = pool.get()?;
            queries::get_labeler_events_after(&mut conn, after_seq, limit)
        })
        .await?
    }

    pub async fn get_latest_seq(&self) -> Result<Option<i64>> {
        let pool = self.pool.clone();
        tokio::task::spawn_blocking(move || {
            let mut conn = pool.get()?;
            queries::get_latest_labeler_seq(&mut conn)
        })
        .await?
    }

    pub async fn get_at_uri_by_event_id(
        &self,
        nostr_event_id: &str,
    ) -> Result<Option<(String, String)>> {
        let pool = self.pool.clone();
        let nostr_event_id = nostr_event_id.to_string();
        tokio::task::spawn_blocking(move || {
            use diesel::prelude::*;
            use divine_bridge_db::schema::record_mappings;

            let mut conn = pool.get()?;
            let result = record_mappings::table
                .filter(record_mappings::nostr_event_id.eq(nostr_event_id))
                .select((record_mappings::at_uri, record_mappings::did))
                .first::<(String, String)>(&mut conn)
                .optional()?;
            Ok(result)
        })
        .await?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use diesel::connection::SimpleConnection;
    use diesel::{Connection, PgConnection};
    use divine_bridge_db::migrations::run_pending_migrations_on;
    use divine_bridge_db::models::NewLabelerEvent;

    // CI and local runs export TEST_DATABASE_URL; require it so every line of
    // this helper executes under coverage (no untaken fallback branch).
    fn test_database_url() -> String {
        std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL must be set for store tests")
    }

    fn migrated_store() -> DbStore {
        let url = test_database_url();
        let mut conn = PgConnection::establish(&url).expect("test database should be reachable");
        // Bridge-owned tables (001) include `record_mappings`; they are idempotent.
        run_pending_migrations_on(&mut conn).expect("bridge migrations should run");
        // `labeler_events` lives in the non-idempotent 002 migration, so reset it
        // with down-then-up the way the crate's integration tests do.
        conn.batch_execute(include_str!(
            "../../../migrations/002_label_tracking/down.sql"
        ))
        .expect("labeler-tracking down migration should run");
        conn.batch_execute(include_str!(
            "../../../migrations/002_label_tracking/up.sql"
        ))
        .expect("labeler-tracking up migration should run");
        DbStore::connect(&url).expect("store connects through the pool")
    }

    #[tokio::test]
    async fn store_inserts_and_reads_labeler_events_and_mappings() {
        let store = migrated_store();

        let event = NewLabelerEvent {
            src_did: "did:plc:test-store-labeler",
            subject_uri: "at://did:plc:user/app.bsky.feed.post/store-test",
            subject_cid: None,
            val: "spam",
            neg: false,
            nostr_event_id: Some("store-test-event"),
            sha256: None,
            origin: "divine",
        };
        let inserted = store
            .insert_labeler_event(&event)
            .await
            .expect("event inserts");
        assert_eq!(inserted.val, "spam");

        let latest = store
            .get_latest_seq()
            .await
            .expect("latest seq query succeeds")
            .expect("at least one event exists after insert");
        assert!(latest >= inserted.seq);

        let events = store
            .get_events_after(0, 1000)
            .await
            .expect("events-after query succeeds");
        assert!(events.iter().any(|e| e.seq == inserted.seq));

        let missing = store
            .get_at_uri_by_event_id("no-such-nostr-event-id")
            .await
            .expect("mapping lookup succeeds");
        assert!(missing.is_none());
    }
}
