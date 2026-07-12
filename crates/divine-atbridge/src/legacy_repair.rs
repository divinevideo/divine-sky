use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Utc};
use diesel::sql_types::{BigInt, Int4, Jsonb, Nullable, Text, Timestamptz};
use diesel::{Connection, PgConnection, QueryableByName, RunQueryDsl};
use divine_bridge_db::models::LegacyBadJwtRepairFilter;
use divine_bridge_db::{
    lock_legacy_badjwt_repair_candidates, preview_legacy_badjwt_repair, revive_legacy_badjwt_jobs,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

pub const BADJWT_SIGNATURE_ERROR: &str = "BadJwt: Signature tag didn't verify";

#[derive(Debug, Clone)]
pub struct LegacyRepairService {
    database_url: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct LegacyRepairPreviewResult {
    pub operation_id: String,
    pub matched_event_ids: Vec<String>,
    pub total_matching: i64,
    pub has_more: bool,
    pub next_after_event_id: Option<String>,
    pub confirmation_digest: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct LegacyRepairApplyResult {
    pub operation_id: String,
    pub changed_count: i64,
    pub skipped_count: i64,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedScope {
    nostr_pubkey: String,
    event_ids: Vec<String>,
    exact_error: Option<String>,
    after_event_id: Option<String>,
    limit: i64,
    matched_event_ids: Vec<String>,
}

#[derive(Debug, QueryableByName)]
struct PersistedAction {
    #[diesel(sql_type = Text)]
    operation_id: String,
    #[diesel(sql_type = Text)]
    actor: String,
    #[diesel(sql_type = Jsonb)]
    scope: Value,
    #[diesel(sql_type = Text)]
    confirmation_digest: String,
    #[diesel(sql_type = Jsonb)]
    before_images: Value,
    #[diesel(sql_type = BigInt)]
    changed_count: i64,
    #[diesel(sql_type = Text)]
    status: String,
    #[diesel(sql_type = Timestamptz)]
    updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SanitizedBeforeImage {
    nostr_event_id: String,
    state: String,
    attempt: i32,
    error_sha256: String,
    lease_owner_present: bool,
    lease_expires_at: Option<DateTime<Utc>>,
    completed_at: Option<DateTime<Utc>>,
    updated_at: DateTime<Utc>,
}

impl LegacyRepairService {
    pub fn new(database_url: String) -> Self {
        Self { database_url }
    }

    fn connect(&self) -> Result<PgConnection> {
        PgConnection::establish(&self.database_url).context("failed to connect to bridge database")
    }

    pub fn preview(
        &self,
        actor: &str,
        mut filter: LegacyBadJwtRepairFilter,
    ) -> Result<LegacyRepairPreviewResult> {
        filter.event_ids.sort();
        filter.event_ids.dedup();
        validate_request(actor, &filter)?;
        let mut conn = self.connect()?;
        let preview = preview_legacy_badjwt_repair(&mut conn, &filter)?;
        let matched_event_ids = preview
            .jobs
            .iter()
            .map(|job| job.nostr_event_id.clone())
            .collect::<Vec<_>>();
        let operation_id = uuid::Uuid::new_v4().to_string();
        let scope = PersistedScope {
            nostr_pubkey: filter.nostr_pubkey,
            event_ids: filter.event_ids,
            exact_error: filter.exact_error,
            after_event_id: filter.after_event_id,
            limit: filter.limit,
            matched_event_ids: matched_event_ids.clone(),
        };
        let scope_json = serde_json::to_value(&scope)?;
        let before_images = sanitized_before_images(&preview.jobs);
        let confirmation_digest =
            confirmation_digest(&operation_id, actor, &scope_json, &before_images)?;

        diesel::sql_query(
            "INSERT INTO operator_actions (
                operation_id, action_type, actor, scope, dry_run,
                confirmation_digest, before_images, matched_count, status
             ) VALUES ($1, 'repair_legacy_badjwt', $2, $3, TRUE, $4, $5, $6, 'previewed')",
        )
        .bind::<Text, _>(&operation_id)
        .bind::<Text, _>(actor)
        .bind::<Jsonb, _>(&scope_json)
        .bind::<Text, _>(&confirmation_digest)
        .bind::<Jsonb, _>(&before_images)
        .bind::<BigInt, _>(matched_event_ids.len() as i64)
        .execute(&mut conn)?;

        Ok(LegacyRepairPreviewResult {
            operation_id,
            matched_event_ids,
            total_matching: preview.total_matching,
            has_more: preview.has_more,
            next_after_event_id: preview.next_after_event_id,
            confirmation_digest,
        })
    }

    pub fn confirm(&self, operation_id: &str, digest: &str) -> Result<LegacyRepairApplyResult> {
        let mut conn = self.connect()?;
        conn.transaction::<_, anyhow::Error, _>(|conn| {
            diesel::sql_query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE").execute(conn)?;
            let action = load_action_for_update(conn, operation_id)?;
            if action.confirmation_digest != digest {
                return Err(anyhow!("confirmation digest does not match preview"));
            }
            if action.status == "applied" {
                return Ok(LegacyRepairApplyResult {
                    operation_id: action.operation_id,
                    changed_count: action.changed_count,
                    skipped_count: 0,
                    status: action.status,
                });
            }
            if action.status != "previewed" {
                return Err(anyhow!("operation is not confirmable"));
            }

            let scope: PersistedScope = serde_json::from_value(action.scope.clone())?;
            let filter = LegacyBadJwtRepairFilter {
                nostr_pubkey: scope.nostr_pubkey,
                event_ids: scope.event_ids,
                exact_error: scope.exact_error,
                after_event_id: scope.after_event_id,
                limit: scope.limit,
            };
            let current = lock_legacy_badjwt_repair_candidates(conn, &filter)?;
            let current_ids = current
                .jobs
                .iter()
                .map(|job| job.nostr_event_id.clone())
                .collect::<Vec<_>>();
            let current_before = sanitized_before_images(&current.jobs);
            let current_digest = confirmation_digest(
                &action.operation_id,
                &action.actor,
                &action.scope,
                &current_before,
            )?;
            if current_digest != action.confirmation_digest {
                return Err(anyhow!("confirmation digest no longer matches candidates"));
            }
            if current_ids != scope.matched_event_ids || current_before != action.before_images {
                return Err(anyhow!("repair candidates changed after preview"));
            }

            let changed =
                revive_legacy_badjwt_jobs(conn, &filter, &scope.matched_event_ids)? as i64;
            if changed != scope.matched_event_ids.len() as i64 {
                return Err(anyhow!("repair changed fewer rows than previewed"));
            }
            diesel::sql_query(
                "UPDATE operator_actions
                 SET dry_run = FALSE, changed_count = $2, status = 'applied', updated_at = NOW()
                 WHERE operation_id = $1",
            )
            .bind::<Text, _>(operation_id)
            .bind::<BigInt, _>(changed)
            .execute(conn)?;

            Ok(LegacyRepairApplyResult {
                operation_id: operation_id.to_string(),
                changed_count: changed,
                skipped_count: 0,
                status: "applied".to_string(),
            })
        })
    }

    pub fn rollback(&self, operation_id: &str) -> Result<LegacyRepairApplyResult> {
        let mut conn = self.connect()?;
        conn.transaction::<_, anyhow::Error, _>(|conn| {
            let action = load_action_for_update(conn, operation_id)?;
            if action.status == "rolled_back" {
                return Ok(LegacyRepairApplyResult {
                    operation_id: action.operation_id,
                    changed_count: action.changed_count,
                    skipped_count: 0,
                    status: action.status,
                });
            }
            if action.status != "applied" {
                return Err(anyhow!("only an applied operation can be rolled back"));
            }
            let before: Vec<SanitizedBeforeImage> =
                serde_json::from_value(action.before_images.clone())?;
            let mut changed = 0i64;
            let mut skipped = 0i64;
            for job in before {
                let rows = diesel::sql_query(
                    "UPDATE publish_jobs
                     SET state = $2, attempt = $3, lease_owner = NULL,
                         lease_expires_at = $4, completed_at = $5, updated_at = $6
                     WHERE nostr_event_id = $1
                       AND state = 'failed'
                       AND completed_at IS NULL
                       AND lease_owner IS NULL
                       AND updated_at = $7",
                )
                .bind::<Text, _>(&job.nostr_event_id)
                .bind::<Text, _>(&job.state)
                .bind::<Int4, _>(job.attempt)
                .bind::<Nullable<Timestamptz>, _>(job.lease_expires_at)
                .bind::<Nullable<Timestamptz>, _>(job.completed_at)
                .bind::<Timestamptz, _>(job.updated_at)
                .bind::<Timestamptz, _>(action.updated_at)
                .execute(conn)?;
                if rows == 0 {
                    skipped += 1;
                    continue;
                }
                changed += 1;
            }
            let status = if skipped == 0 {
                "rolled_back"
            } else {
                "rollback_partial"
            };
            diesel::sql_query(
                "UPDATE operator_actions
                 SET status = $2, changed_count = $3, updated_at = NOW()
                 WHERE operation_id = $1",
            )
            .bind::<Text, _>(operation_id)
            .bind::<Text, _>(status)
            .bind::<BigInt, _>(changed)
            .execute(conn)?;
            Ok(LegacyRepairApplyResult {
                operation_id: operation_id.to_string(),
                changed_count: changed,
                skipped_count: skipped,
                status: status.to_string(),
            })
        })
    }
}

fn load_action_for_update(conn: &mut PgConnection, operation_id: &str) -> Result<PersistedAction> {
    diesel::sql_query(
        "SELECT operation_id, scope, confirmation_digest, before_images,
                actor, changed_count, status, updated_at
         FROM operator_actions
         WHERE operation_id = $1
         FOR UPDATE",
    )
    .bind::<Text, _>(operation_id)
    .get_result::<PersistedAction>(conn)
    .context("repair operation not found")
}

fn validate_request(actor: &str, filter: &LegacyBadJwtRepairFilter) -> Result<()> {
    if actor.trim().is_empty() {
        return Err(anyhow!("actor is required"));
    }
    if !is_lower_hex_64(&filter.nostr_pubkey) {
        return Err(anyhow!("nostr pubkey must be 64 lowercase hex characters"));
    }
    if filter.limit < 1 || filter.limit > 1_000 {
        return Err(anyhow!("repair limit must be between 1 and 1000"));
    }
    if filter.event_ids.is_empty() && filter.exact_error.is_none() {
        return Err(anyhow!("event IDs or exact BadJwt mode are required"));
    }
    if !filter.event_ids.is_empty() && filter.exact_error.is_some() {
        return Err(anyhow!(
            "explicit event IDs cannot be combined with BadJwt class mode"
        ));
    }
    if filter.event_ids.iter().any(|id| !is_lower_hex_64(id)) {
        return Err(anyhow!("event IDs must be 64 lowercase hex characters"));
    }
    if let Some(after) = &filter.after_event_id {
        if !is_lower_hex_64(after) {
            return Err(anyhow!(
                "after event ID must be 64 lowercase hex characters"
            ));
        }
    }
    if let Some(error) = &filter.exact_error {
        if error != BADJWT_SIGNATURE_ERROR {
            return Err(anyhow!(
                "only the exact allowlisted BadJwt error is accepted"
            ));
        }
    }
    Ok(())
}

fn is_lower_hex_64(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn confirmation_digest(
    operation_id: &str,
    actor: &str,
    scope: &Value,
    before_images: &Value,
) -> Result<String> {
    let canonical = serde_json::to_vec(&json!({
        "operation_id": operation_id,
        "actor": actor,
        "scope": scope,
        "before_images": before_images,
    }))?;
    Ok(hex::encode(Sha256::digest(canonical)))
}

fn sanitized_before_images(jobs: &[divine_bridge_db::models::LegacyRepairJobSnapshot]) -> Value {
    serde_json::to_value(
        jobs.iter()
            .map(|job| SanitizedBeforeImage {
                nostr_event_id: job.nostr_event_id.clone(),
                state: job.state.clone(),
                attempt: job.attempt,
                error_sha256: hex::encode(Sha256::digest(
                    job.error.as_deref().unwrap_or_default().as_bytes(),
                )),
                lease_owner_present: job.lease_owner.is_some(),
                lease_expires_at: job.lease_expires_at,
                completed_at: job.completed_at,
                updated_at: job.updated_at,
            })
            .collect::<Vec<_>>(),
    )
    .expect("sanitized before-images are serializable")
}
