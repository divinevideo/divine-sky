use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::routing::{get, post};
use axum::Router;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub mod routes;

pub type SharedLinks = Arc<Mutex<HashMap<String, AccountLinkRecord>>>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProvisioningState {
    Pending,
    Ready,
    Failed,
    Disabled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AccountLinkRecord {
    pub nostr_pubkey: String,
    pub handle: String,
    pub did: Option<String>,
    pub provisioning_state: ProvisioningState,
    pub provisioning_error: Option<String>,
    pub disabled_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Default)]
pub struct AppState {
    links: SharedLinks,
}

impl AppState {
    pub(crate) fn upsert_pending(&self, nostr_pubkey: String, handle: String) -> AccountLinkRecord {
        let mut links = self.links.lock().unwrap();
        let now = Utc::now();
        let record = links
            .entry(nostr_pubkey.clone())
            .and_modify(|existing| {
                existing.handle = handle.clone();
                existing.provisioning_state = ProvisioningState::Pending;
                existing.provisioning_error = None;
                existing.disabled_at = None;
                existing.updated_at = now;
            })
            .or_insert_with(|| AccountLinkRecord {
                nostr_pubkey: nostr_pubkey.clone(),
                handle: handle.clone(),
                did: None,
                provisioning_state: ProvisioningState::Pending,
                provisioning_error: None,
                disabled_at: None,
                created_at: now,
                updated_at: now,
            });
        record.clone()
    }

    pub(crate) fn upsert_ready(
        &self,
        nostr_pubkey: String,
        handle: String,
        did: String,
    ) -> AccountLinkRecord {
        let mut links = self.links.lock().unwrap();
        let now = Utc::now();
        let record = links
            .entry(nostr_pubkey.clone())
            .and_modify(|existing| {
                existing.handle = handle.clone();
                existing.did = Some(did.clone());
                existing.provisioning_state = ProvisioningState::Ready;
                existing.provisioning_error = None;
                existing.disabled_at = None;
                existing.updated_at = now;
            })
            .or_insert_with(|| AccountLinkRecord {
                nostr_pubkey: nostr_pubkey.clone(),
                handle: handle.clone(),
                did: Some(did.clone()),
                provisioning_state: ProvisioningState::Ready,
                provisioning_error: None,
                disabled_at: None,
                created_at: now,
                updated_at: now,
            });
        record.clone()
    }

    pub(crate) fn get_by_pubkey(&self, nostr_pubkey: &str) -> Option<AccountLinkRecord> {
        self.links.lock().unwrap().get(nostr_pubkey).cloned()
    }

    pub(crate) fn disable_by_pubkey(&self, nostr_pubkey: &str) -> Option<AccountLinkRecord> {
        let mut links = self.links.lock().unwrap();
        let record = links.get_mut(nostr_pubkey)?;
        let now = Utc::now();
        record.provisioning_state = ProvisioningState::Disabled;
        record.disabled_at = Some(now);
        record.updated_at = now;
        Some(record.clone())
    }

    pub(crate) fn get_by_handle(&self, handle: &str) -> Option<AccountLinkRecord> {
        self.links
            .lock()
            .unwrap()
            .values()
            .find(|record| record.handle == handle)
            .cloned()
    }
}

pub fn app() -> Router {
    let state = AppState::default();

    Router::new()
        .route("/api/account-links/opt-in", post(routes::opt_in::handler))
        .route(
            "/api/account-links/provision",
            post(routes::provision::handler),
        )
        .route(
            "/api/account-links/:nostr_pubkey/status",
            get(routes::status::handler),
        )
        .route(
            "/api/account-links/:nostr_pubkey/disable",
            post(routes::disable::handler),
        )
        .route(
            "/api/account-links/:nostr_pubkey/export",
            get(routes::export::handler),
        )
        .route("/.well-known/atproto-did", get(routes::well_known::handler))
        .with_state(state)
}
