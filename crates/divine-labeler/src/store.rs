//! Database store wrapping divine-bridge-db queries for the labeler.

use anyhow::Result;

use divine_bridge_db::models::{LabelerEvent, NewLabelerEvent};
use divine_bridge_db::pool::{build_pool, DbPool};
use divine_bridge_db::queries;

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

    pub fn insert_labeler_event(&self, event: &NewLabelerEvent) -> Result<LabelerEvent> {
        let mut conn = self.pool.get()?;
        queries::insert_labeler_event(&mut conn, event)
    }

    pub fn get_events_after(&self, after_seq: i64, limit: i64) -> Result<Vec<LabelerEvent>> {
        let mut conn = self.pool.get()?;
        queries::get_labeler_events_after(&mut conn, after_seq, limit)
    }

    pub fn get_latest_seq(&self) -> Result<Option<i64>> {
        let mut conn = self.pool.get()?;
        queries::get_latest_labeler_seq(&mut conn)
    }

    pub fn get_at_uri_by_event_id(&self, nostr_event_id: &str) -> Result<Option<(String, String)>> {
        use diesel::prelude::*;
        use divine_bridge_db::schema::record_mappings;

        let mut conn = self.pool.get()?;
        let result = record_mappings::table
            .filter(record_mappings::nostr_event_id.eq(nostr_event_id))
            .select((record_mappings::at_uri, record_mappings::did))
            .first::<(String, String)>(&mut conn)
            .optional()?;
        Ok(result)
    }
}
