//! Database store wrapping divine-bridge-db queries for the labeler.

use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use diesel::Connection;
use diesel::PgConnection;

use divine_bridge_db::models::{LabelerEvent, NewLabelerEvent};
use divine_bridge_db::queries;

type SharedConnection = Arc<Mutex<PgConnection>>;

#[derive(Clone)]
pub struct DbStore {
    connection: SharedConnection,
}

impl DbStore {
    pub fn connect(database_url: &str) -> Result<Self> {
        let connection =
            PgConnection::establish(database_url).context("failed to connect to PostgreSQL")?;
        Ok(Self {
            connection: Arc::new(Mutex::new(connection)),
        })
    }

    pub fn insert_labeler_event(&self, event: &NewLabelerEvent) -> Result<LabelerEvent> {
        let mut conn = self.connection.lock().unwrap();
        queries::insert_labeler_event(&mut conn, event)
    }

    pub fn get_events_after(&self, after_seq: i64, limit: i64) -> Result<Vec<LabelerEvent>> {
        let mut conn = self.connection.lock().unwrap();
        queries::get_labeler_events_after(&mut conn, after_seq, limit)
    }

    pub fn get_latest_seq(&self) -> Result<Option<i64>> {
        let mut conn = self.connection.lock().unwrap();
        queries::get_latest_labeler_seq(&mut conn)
    }

    pub fn get_at_uri_by_event_id(&self, nostr_event_id: &str) -> Result<Option<(String, String)>> {
        use divine_bridge_db::schema::record_mappings;
        use diesel::prelude::*;

        let mut conn = self.connection.lock().unwrap();
        let result = record_mappings::table
            .filter(record_mappings::nostr_event_id.eq(nostr_event_id))
            .select((record_mappings::at_uri, record_mappings::did))
            .first::<(String, String)>(&mut *conn)
            .optional()?;
        Ok(result)
    }
}
