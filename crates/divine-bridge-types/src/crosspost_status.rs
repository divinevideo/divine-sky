use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{PublishState, RecordStatus};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CrosspostStatus {
    NotApplicable,
    Queued,
    Publishing,
    Published,
    Retrying,
    Failed,
    Removed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CrosspostAccountSummary {
    pub crosspost_enabled: bool,
    pub provisioning_state: String,
    pub did: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CrosspostStatusResponse {
    pub account: CrosspostAccountSummary,
    pub videos: Vec<CrosspostVideoStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CrosspostVideoStatus {
    pub nostr_event_id: String,
    pub status: CrosspostStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub at_uri: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure: Option<CrosspostFailure>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CrosspostFailure {
    pub reason: CrosspostFailureReason,
    pub retryable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_attempt_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CrosspostFailureReason {
    Quota,
    Unsupported,
    Internal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrosspostAccountContext {
    pub crosspost_enabled: bool,
    pub provisioning_state: String,
    pub did: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrosspostJobContext {
    pub state: String,
    pub error: Option<String>,
    pub lease_expires_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub updated_at: DateTime<Utc>,
    pub at_uri: Option<String>,
    pub cid: Option<String>,
    pub record_status: Option<String>,
}

impl CrosspostAccountSummary {
    pub fn from_context(account: Option<&CrosspostAccountContext>) -> Self {
        match account {
            Some(account) => Self {
                crosspost_enabled: account.crosspost_enabled,
                provisioning_state: account.provisioning_state.clone(),
                did: account.did.clone(),
            },
            None => Self {
                crosspost_enabled: false,
                provisioning_state: "missing".to_string(),
                did: None,
            },
        }
    }
}

pub fn derive_crosspost_status(
    nostr_event_id: String,
    account: Option<&CrosspostAccountContext>,
    job: Option<&CrosspostJobContext>,
) -> CrosspostVideoStatus {
    if !account_is_ready(account) {
        return not_applicable(nostr_event_id);
    }

    let Some(job) = job else {
        return not_applicable(nostr_event_id);
    };

    let status = match job.state.as_str() {
        state if state == PublishState::Pending.as_str() => CrosspostVideoStatus {
            nostr_event_id,
            status: CrosspostStatus::Queued,
            at_uri: None,
            cid: None,
            failure: None,
            updated_at: Some(job.updated_at),
        },
        state if state == PublishState::InProgress.as_str() => CrosspostVideoStatus {
            nostr_event_id,
            status: CrosspostStatus::Publishing,
            at_uri: None,
            cid: None,
            failure: None,
            updated_at: Some(job.updated_at),
        },
        state if state == PublishState::Published.as_str() => {
            if job.record_status.as_deref() == Some(RecordStatus::Published.as_str())
                && job.at_uri.is_some()
            {
                CrosspostVideoStatus {
                    nostr_event_id,
                    status: CrosspostStatus::Published,
                    at_uri: job.at_uri.clone(),
                    cid: job.cid.clone(),
                    failure: None,
                    updated_at: Some(job.updated_at),
                }
            } else {
                removed(nostr_event_id, job.updated_at)
            }
        }
        state if state == PublishState::Failed.as_str() && job.completed_at.is_none() => {
            CrosspostVideoStatus {
                nostr_event_id,
                status: CrosspostStatus::Retrying,
                at_uri: None,
                cid: None,
                failure: Some(CrosspostFailure {
                    reason: classify_failure_reason(job.error.as_deref()),
                    retryable: true,
                    next_attempt_at: job.lease_expires_at,
                }),
                updated_at: Some(job.updated_at),
            }
        }
        state if state == PublishState::Failed.as_str() => CrosspostVideoStatus {
            nostr_event_id,
            status: CrosspostStatus::Failed,
            at_uri: None,
            cid: None,
            failure: Some(CrosspostFailure {
                reason: classify_failure_reason(job.error.as_deref()),
                retryable: false,
                next_attempt_at: None,
            }),
            updated_at: Some(job.updated_at),
        },
        state if state == PublishState::Skipped.as_str() => removed(nostr_event_id, job.updated_at),
        _ => CrosspostVideoStatus {
            nostr_event_id,
            status: CrosspostStatus::Failed,
            at_uri: None,
            cid: None,
            failure: Some(CrosspostFailure {
                reason: CrosspostFailureReason::Internal,
                retryable: false,
                next_attempt_at: None,
            }),
            updated_at: Some(job.updated_at),
        },
    };

    status
}

fn account_is_ready(account: Option<&CrosspostAccountContext>) -> bool {
    matches!(
        account,
        Some(CrosspostAccountContext {
            crosspost_enabled: true,
            provisioning_state,
            ..
        }) if provisioning_state == "ready"
    )
}

fn not_applicable(nostr_event_id: String) -> CrosspostVideoStatus {
    CrosspostVideoStatus {
        nostr_event_id,
        status: CrosspostStatus::NotApplicable,
        at_uri: None,
        cid: None,
        failure: None,
        updated_at: None,
    }
}

fn removed(nostr_event_id: String, updated_at: DateTime<Utc>) -> CrosspostVideoStatus {
    CrosspostVideoStatus {
        nostr_event_id,
        status: CrosspostStatus::Removed,
        at_uri: None,
        cid: None,
        failure: None,
        updated_at: Some(updated_at),
    }
}

fn classify_failure_reason(error: Option<&str>) -> CrosspostFailureReason {
    let Some(error) = error else {
        return CrosspostFailureReason::Internal;
    };
    if error.contains("daily_vid_limit_exceeded") {
        CrosspostFailureReason::Quota
    } else if error.to_ascii_lowercase().contains("unsupported") {
        CrosspostFailureReason::Unsupported
    } else {
        CrosspostFailureReason::Internal
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    const EVENT_ID: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    fn ready_account() -> CrosspostAccountContext {
        CrosspostAccountContext {
            crosspost_enabled: true,
            provisioning_state: "ready".to_string(),
            did: Some("did:plc:alice".to_string()),
        }
    }

    fn base_job(state: &str) -> CrosspostJobContext {
        CrosspostJobContext {
            state: state.to_string(),
            error: None,
            lease_expires_at: None,
            completed_at: None,
            updated_at: Utc.with_ymd_and_hms(2026, 8, 4, 20, 0, 0).unwrap(),
            at_uri: None,
            cid: None,
            record_status: None,
        }
    }

    #[test]
    fn missing_account_reports_not_applicable() {
        let status =
            derive_crosspost_status(EVENT_ID.to_string(), None, Some(&base_job("pending")));
        assert_eq!(status.status, CrosspostStatus::NotApplicable);
        assert!(status.updated_at.is_none());
    }

    #[test]
    fn missing_account_summary_reports_missing_state() {
        let summary = CrosspostAccountSummary::from_context(None);
        assert_eq!(
            summary,
            CrosspostAccountSummary {
                crosspost_enabled: false,
                provisioning_state: "missing".to_string(),
                did: None,
            }
        );
    }

    #[test]
    fn ready_account_summary_preserves_account_fields() {
        let account = ready_account();
        let summary = CrosspostAccountSummary::from_context(Some(&account));
        assert_eq!(
            summary,
            CrosspostAccountSummary {
                crosspost_enabled: true,
                provisioning_state: "ready".to_string(),
                did: Some("did:plc:alice".to_string()),
            }
        );
    }

    #[test]
    fn disabled_crosspost_reports_not_applicable() {
        let mut account = ready_account();
        account.crosspost_enabled = false;
        let status = derive_crosspost_status(
            EVENT_ID.to_string(),
            Some(&account),
            Some(&base_job("pending")),
        );
        assert_eq!(status.status, CrosspostStatus::NotApplicable);
    }

    #[test]
    fn pending_job_reports_queued() {
        let status = derive_crosspost_status(
            EVENT_ID.to_string(),
            Some(&ready_account()),
            Some(&base_job("pending")),
        );
        assert_eq!(status.status, CrosspostStatus::Queued);
    }

    #[test]
    fn in_progress_job_reports_publishing() {
        let status = derive_crosspost_status(
            EVENT_ID.to_string(),
            Some(&ready_account()),
            Some(&base_job("in_progress")),
        );
        assert_eq!(status.status, CrosspostStatus::Publishing);
    }

    #[test]
    fn published_mapping_reports_published_with_atproto_ids() {
        let mut job = base_job("published");
        job.at_uri = Some("at://did:plc:alice/app.bsky.feed.post/rkey".to_string());
        job.cid = Some("bafyreialice".to_string());
        job.record_status = Some("published".to_string());
        let status =
            derive_crosspost_status(EVENT_ID.to_string(), Some(&ready_account()), Some(&job));
        assert_eq!(status.status, CrosspostStatus::Published);
        assert_eq!(status.at_uri, job.at_uri);
        assert_eq!(status.cid, job.cid);
    }

    #[test]
    fn quota_parked_job_reports_retrying_not_failed() {
        let mut job = base_job("failed");
        job.error = Some("daily_vid_limit_exceeded".to_string());
        job.lease_expires_at = Some(Utc.with_ymd_and_hms(2026, 8, 4, 21, 0, 0).unwrap());
        let status =
            derive_crosspost_status(EVENT_ID.to_string(), Some(&ready_account()), Some(&job));
        assert_eq!(status.status, CrosspostStatus::Retrying);
        assert_eq!(
            status.failure,
            Some(CrosspostFailure {
                reason: CrosspostFailureReason::Quota,
                retryable: true,
                next_attempt_at: job.lease_expires_at,
            })
        );
    }

    #[test]
    fn unsupported_failure_maps_to_closed_reason() {
        let mut job = base_job("failed");
        job.error = Some("unsupported event kind: 1".to_string());
        job.completed_at = Some(Utc.with_ymd_and_hms(2026, 8, 4, 22, 0, 0).unwrap());
        let status =
            derive_crosspost_status(EVENT_ID.to_string(), Some(&ready_account()), Some(&job));
        assert_eq!(
            status.failure,
            Some(CrosspostFailure {
                reason: CrosspostFailureReason::Unsupported,
                retryable: false,
                next_attempt_at: None,
            })
        );
    }

    #[test]
    fn attempt_exhaustion_reports_non_retryable_failure() {
        let mut job = base_job("failed");
        job.error = Some("upstream body containing private detail".to_string());
        job.completed_at = Some(Utc.with_ymd_and_hms(2026, 8, 4, 22, 0, 0).unwrap());
        let status =
            derive_crosspost_status(EVENT_ID.to_string(), Some(&ready_account()), Some(&job));
        assert_eq!(status.status, CrosspostStatus::Failed);
        assert_eq!(
            status.failure,
            Some(CrosspostFailure {
                reason: CrosspostFailureReason::Internal,
                retryable: false,
                next_attempt_at: None,
            })
        );
    }

    #[test]
    fn failed_job_without_error_reports_internal_reason() {
        let mut job = base_job("failed");
        job.completed_at = Some(Utc.with_ymd_and_hms(2026, 8, 4, 22, 0, 0).unwrap());
        let status =
            derive_crosspost_status(EVENT_ID.to_string(), Some(&ready_account()), Some(&job));
        assert_eq!(
            status.failure,
            Some(CrosspostFailure {
                reason: CrosspostFailureReason::Internal,
                retryable: false,
                next_attempt_at: None,
            })
        );
    }

    #[test]
    fn skipped_job_reports_removed() {
        let status = derive_crosspost_status(
            EVENT_ID.to_string(),
            Some(&ready_account()),
            Some(&base_job("skipped")),
        );
        assert_eq!(status.status, CrosspostStatus::Removed);
    }

    #[test]
    fn non_published_mapping_reports_removed() {
        let mut job = base_job("published");
        job.at_uri = Some("at://did:plc:alice/app.bsky.feed.post/rkey".to_string());
        job.record_status = Some("deleted".to_string());
        let status =
            derive_crosspost_status(EVENT_ID.to_string(), Some(&ready_account()), Some(&job));
        assert_eq!(status.status, CrosspostStatus::Removed);
    }

    #[test]
    fn unknown_job_state_reports_internal_failure() {
        let status = derive_crosspost_status(
            EVENT_ID.to_string(),
            Some(&ready_account()),
            Some(&base_job("mystery")),
        );
        assert_eq!(status.status, CrosspostStatus::Failed);
        assert_eq!(
            status.failure,
            Some(CrosspostFailure {
                reason: CrosspostFailureReason::Internal,
                retryable: false,
                next_attempt_at: None,
            })
        );
    }
}
