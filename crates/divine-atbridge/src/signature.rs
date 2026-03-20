//! Nostr event signature verification using BIP-340 Schnorr signatures.

use anyhow::{Context, Result};
use divine_bridge_types::NostrEvent;
use secp256k1::schnorr::Signature;
use secp256k1::{Message, Secp256k1, XOnlyPublicKey};
use sha2::{Digest, Sha256};

/// Verify a Nostr event's Schnorr signature.
///
/// Checks that:
/// 1. The event `id` matches the SHA-256 of the canonical serialization
///    `[0, pubkey, created_at, kind, tags, content]`
/// 2. The `sig` is a valid BIP-340 Schnorr signature over the event id
///    using `pubkey`.
pub fn verify_nostr_event(event: &NostrEvent) -> Result<bool> {
    let secp = Secp256k1::verification_only();

    // 1. Recompute the event id from canonical serialization
    let canonical = serde_json::json!([
        0,
        &event.pubkey,
        event.created_at,
        event.kind,
        &event.tags,
        &event.content
    ]);
    let canonical_str =
        serde_json::to_string(&canonical).context("failed to serialize canonical event")?;

    let mut hasher = Sha256::new();
    hasher.update(canonical_str.as_bytes());
    let computed_id: [u8; 32] = hasher.finalize().into();
    let computed_id_hex = hex::encode(computed_id);

    // 2. Check that the provided id matches the computed one
    if computed_id_hex != event.id {
        return Ok(false);
    }

    // 3. Parse the pubkey and signature from hex
    let pubkey_bytes = hex::decode(&event.pubkey).context("invalid pubkey hex")?;
    let xonly = XOnlyPublicKey::from_slice(&pubkey_bytes).context("invalid pubkey")?;

    let sig_bytes = hex::decode(&event.sig).context("invalid signature hex")?;
    let sig = Signature::from_slice(&sig_bytes).context("invalid signature")?;

    // 4. Verify the BIP-340 Schnorr signature
    let msg = Message::from_digest(computed_id);
    match secp.verify_schnorr(&sig, &msg, &xonly) {
        Ok(()) => Ok(true),
        Err(_) => Ok(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use secp256k1::rand::rngs::OsRng;
    use secp256k1::{Keypair, Secp256k1};

    /// Helper: build a valid signed Nostr event from scratch.
    fn make_signed_event(
        kind: u64,
        content: &str,
        tags: Vec<Vec<String>>,
    ) -> (NostrEvent, Keypair) {
        let secp = Secp256k1::new();
        let keypair = Keypair::new(&secp, &mut OsRng);
        let (xonly, _parity) = keypair.x_only_public_key();
        let pubkey_hex = hex::encode(xonly.serialize());

        let created_at: i64 = 1_700_000_000;

        // Canonical serialization: [0, pubkey, created_at, kind, tags, content]
        let canonical = serde_json::json!([0, pubkey_hex, created_at, kind, tags, content]);
        let canonical_bytes = serde_json::to_string(&canonical).unwrap();

        let mut hasher = Sha256::new();
        hasher.update(canonical_bytes.as_bytes());
        let id_bytes: [u8; 32] = hasher.finalize().into();
        let id_hex = hex::encode(id_bytes);

        let msg = Message::from_digest(id_bytes);
        let sig = secp.sign_schnorr(&msg, &keypair);
        let sig_hex = hex::encode(sig.serialize());

        let event = NostrEvent {
            id: id_hex,
            pubkey: pubkey_hex,
            created_at,
            kind,
            tags,
            content: content.to_string(),
            sig: sig_hex,
        };

        (event, keypair)
    }

    #[test]
    fn test_valid_event_verifies() {
        let (event, _kp) = make_signed_event(1, "hello world", vec![]);
        let result = verify_nostr_event(&event).expect("should not error");
        assert!(result, "valid event should verify");
    }

    #[test]
    fn test_invalid_signature_fails() {
        let (mut event, _kp) = make_signed_event(1, "hello world", vec![]);
        // Corrupt the signature
        let mut sig_bytes = hex::decode(&event.sig).unwrap();
        sig_bytes[0] ^= 0xff;
        event.sig = hex::encode(&sig_bytes);

        let result = verify_nostr_event(&event);
        // Should either return Ok(false) or an error
        if let Ok(valid) = result {
            assert!(!valid, "corrupted sig should not verify");
        }
    }

    #[test]
    fn test_mismatched_pubkey_fails() {
        let (mut event, _kp) = make_signed_event(1, "hello world", vec![]);
        // Replace pubkey with a different key
        let secp = Secp256k1::new();
        let other_kp = Keypair::new(&secp, &mut OsRng);
        let (other_xonly, _) = other_kp.x_only_public_key();
        event.pubkey = hex::encode(other_xonly.serialize());

        let result = verify_nostr_event(&event);
        if let Ok(valid) = result {
            assert!(!valid, "wrong pubkey should not verify");
        }
    }

    #[test]
    fn test_tampered_content_fails() {
        let (mut event, _kp) = make_signed_event(1, "hello world", vec![]);
        event.content = "tampered content".to_string();

        let result = verify_nostr_event(&event);
        if let Ok(valid) = result {
            assert!(!valid, "tampered content should not verify");
        }
    }

    #[test]
    fn test_event_with_tags() {
        let tags = vec![
            vec!["e".to_string(), "abc123".to_string()],
            vec!["p".to_string(), "def456".to_string()],
        ];
        let (event, _kp) = make_signed_event(1, "tagged event", tags);
        let result = verify_nostr_event(&event).expect("should not error");
        assert!(result, "event with tags should verify");
    }
}
