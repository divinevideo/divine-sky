pub mod vocabulary;
pub mod outbound;
pub mod labeler_service;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SubjectKind {
    Post,
    Account,
    Blob,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModerationAction {
    pub subject: SubjectKind,
    pub subject_id: String,
    pub action: String,
    pub reason: Option<String>,
    pub inbound: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DivineLabel {
    pub subject_id: String,
    pub value: String,
    pub neg: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModerationQueueEntry {
    pub subject_id: String,
    pub action: String,
    pub review_state: String,
    pub reason: Option<String>,
}

pub fn map_action_to_label(action: &ModerationAction) -> Option<DivineLabel> {
    let value = match action.action.as_str() {
        "nsfw" => "divine-adult",
        "spam" => "divine-spam",
        "copyright" => "divine-copyright",
        "self-harm" => "divine-self-harm",
        _ => return None,
    };

    Some(DivineLabel {
        subject_id: action.subject_id.clone(),
        value: value.to_string(),
        neg: false,
    })
}

pub fn queue_inbound_moderation(action: &ModerationAction) -> ModerationQueueEntry {
    ModerationQueueEntry {
        subject_id: action.subject_id.clone(),
        action: action.action.clone(),
        review_state: "pending-human-review".to_string(),
        reason: action.reason.clone(),
    }
}
