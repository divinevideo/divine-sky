/// Bidirectional label vocabulary mapping between DiVine, ATProto, and NIP-32 (Nostr).
///
/// Each entry describes how a moderation concept is represented across all three
/// protocol families so the bridge can translate inbound and outbound labels.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VocabEntry {
    /// Canonical DiVine label value (our internal representation).
    pub divine: &'static str,
    /// Corresponding ATProto label value (com.atproto.label.defs#label).
    pub atproto: &'static str,
    /// Optional NIP-32 label value. `None` when the concept doesn't map to a
    /// NIP-32 label (e.g. takedown is NIP-09 delete, not a label).
    pub nip32: Option<&'static str>,
    /// NIP-32 namespace (`L` tag value) when nip32 is Some.
    pub nip32_namespace: &'static str,
    /// Whether the label requires server-side enforcement (take-down, suspend)
    /// as opposed to advisory/client-side display.
    pub requires_enforcement: bool,
}

pub const VOCABULARY: &[VocabEntry] = &[
    // ── Content labels ───────────────────────────────────────────────
    VocabEntry {
        divine: "nudity",
        atproto: "nudity",
        nip32: Some("nudity"),
        nip32_namespace: "content-warning",
        requires_enforcement: false,
    },
    VocabEntry {
        divine: "sexual",
        atproto: "sexual",
        nip32: Some("sexual"),
        nip32_namespace: "content-warning",
        requires_enforcement: false,
    },
    VocabEntry {
        divine: "porn",
        atproto: "porn",
        nip32: Some("porn"),
        nip32_namespace: "content-warning",
        requires_enforcement: false,
    },
    VocabEntry {
        divine: "graphic-media",
        atproto: "graphic-media",
        nip32: Some("graphic-media"),
        nip32_namespace: "content-warning",
        requires_enforcement: false,
    },
    VocabEntry {
        divine: "violence",
        atproto: "violence",
        nip32: Some("violence"),
        nip32_namespace: "content-warning",
        requires_enforcement: false,
    },
    VocabEntry {
        divine: "self-harm",
        atproto: "self-harm",
        nip32: Some("self-harm"),
        nip32_namespace: "content-warning",
        requires_enforcement: false,
    },
    // ── Synthetic / AI labels ────────────────────────────────────────
    VocabEntry {
        divine: "ai-generated",
        atproto: "ai-generated",
        nip32: Some("ai-generated"),
        nip32_namespace: "content-warning",
        requires_enforcement: false,
    },
    VocabEntry {
        divine: "deepfake",
        atproto: "deepfake",
        nip32: Some("deepfake"),
        nip32_namespace: "content-warning",
        requires_enforcement: false,
    },
    // ── Behavioral labels ────────────────────────────────────────────
    VocabEntry {
        divine: "spam",
        atproto: "spam",
        nip32: Some("spam"),
        nip32_namespace: "content-warning",
        requires_enforcement: false,
    },
    VocabEntry {
        divine: "hate",
        atproto: "hate",
        nip32: Some("hate"),
        nip32_namespace: "content-warning",
        requires_enforcement: false,
    },
    VocabEntry {
        divine: "harassment",
        atproto: "harassment",
        nip32: Some("harassment"),
        nip32_namespace: "content-warning",
        requires_enforcement: false,
    },
    // ── Enforcement labels ───────────────────────────────────────────
    VocabEntry {
        divine: "takedown",
        atproto: "!takedown",
        nip32: None,
        nip32_namespace: "",
        requires_enforcement: true,
    },
    VocabEntry {
        divine: "suspend",
        atproto: "!suspend",
        nip32: None,
        nip32_namespace: "",
        requires_enforcement: true,
    },
    VocabEntry {
        divine: "content-warning",
        atproto: "!warn",
        nip32: None,
        nip32_namespace: "",
        requires_enforcement: true,
    },
];

/// Aliases for inbound ATProto labels that should be normalized before lookup.
const INBOUND_ALIASES: &[(&str, &str)] = &[("gore", "graphic-media")];

/// Map an ATProto label value to its canonical DiVine label.
pub fn atproto_to_divine(atproto: &str) -> Option<&'static str> {
    // Check aliases first.
    let normalized = INBOUND_ALIASES
        .iter()
        .find(|(alias, _)| *alias == atproto)
        .map(|(_, target)| *target)
        .unwrap_or(atproto);

    VOCABULARY
        .iter()
        .find(|e| e.atproto == normalized)
        .map(|e| e.divine)
}

/// Map a DiVine label to its ATProto representation.
pub fn divine_to_atproto(divine: &str) -> Option<&'static str> {
    VOCABULARY
        .iter()
        .find(|e| e.divine == divine)
        .map(|e| e.atproto)
}

/// Map a DiVine label to a NIP-32 (namespace, value) pair.
/// Returns `None` when the concept doesn't translate to a NIP-32 label
/// (e.g. takedown uses NIP-09 deletion instead).
pub fn divine_to_nip32(divine: &str) -> Option<(&'static str, &'static str)> {
    VOCABULARY
        .iter()
        .find(|e| e.divine == divine)
        .and_then(|e| e.nip32.map(|v| (e.nip32_namespace, v)))
}

/// Look up a vocabulary entry by its ATProto label value.
pub fn get_entry_by_atproto(atproto: &str) -> Option<&'static VocabEntry> {
    let normalized = INBOUND_ALIASES
        .iter()
        .find(|(alias, _)| *alias == atproto)
        .map(|(_, target)| *target)
        .unwrap_or(atproto);

    VOCABULARY.iter().find(|e| e.atproto == normalized)
}

/// Check whether a DiVine label requires server-side enforcement.
pub fn requires_enforcement(divine: &str) -> bool {
    VOCABULARY
        .iter()
        .find(|e| e.divine == divine)
        .map(|e| e.requires_enforcement)
        .unwrap_or(false)
}
