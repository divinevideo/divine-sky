//! ATProto label signing using DAG-CBOR + ECDSA (ES256K).
//!
//! ATProto labels are signed by:
//! 1. Constructing a map of label fields (excluding `sig`) in sorted key order
//! 2. Encoding as DAG-CBOR
//! 3. Signing the CBOR bytes with ECDSA over secp256k1 (low-S normalized)
//! 4. Base64-encoding the 64-byte compact signature

use anyhow::{Context, Result};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use k256::ecdsa::{signature::Signer, Signature, SigningKey};
use serde::Serialize;

/// Label fields to be signed (no `sig` field).
#[derive(Debug, Clone, Serialize)]
pub struct UnsignedLabel {
    pub ver: u32,
    pub src: String,
    pub uri: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cid: Option<String>,
    pub val: String,
    pub neg: bool,
    pub cts: String,
}

/// Sign an unsigned label, returning a base64-encoded ECDSA signature.
pub fn sign_label(label: &UnsignedLabel, key: &SigningKey) -> Result<String> {
    let cbor_bytes =
        serde_ipld_dagcbor::to_vec(label).context("failed to encode label as DAG-CBOR")?;
    let signature: Signature = key.sign(&cbor_bytes);
    Ok(BASE64.encode(signature.to_bytes()))
}

/// Parse a hex-encoded secp256k1 private key into a SigningKey.
pub fn signing_key_from_hex(hex_key: &str) -> Result<SigningKey> {
    let bytes = hex::decode(hex_key).context("invalid hex in signing key")?;
    SigningKey::from_bytes(bytes.as_slice().into()).context("invalid secp256k1 private key")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_keypair() -> (SigningKey, String) {
        let key_bytes = [1u8; 32];
        let signing_key = SigningKey::from_bytes(&key_bytes.into()).unwrap();
        let hex_key = hex::encode(key_bytes);
        (signing_key, hex_key)
    }

    #[test]
    fn sign_label_produces_base64_signature() {
        let (signing_key, _) = test_keypair();
        let label = UnsignedLabel {
            ver: 1,
            src: "did:plc:test-labeler".to_string(),
            uri: "at://did:plc:user1/app.bsky.feed.post/rkey1".to_string(),
            cid: None,
            val: "nudity".to_string(),
            neg: false,
            cts: "2026-03-20T12:00:00.000Z".to_string(),
        };
        let sig = sign_label(&label, &signing_key).unwrap();
        assert!(!sig.is_empty());
        let decoded = BASE64.decode(&sig).unwrap();
        assert_eq!(decoded.len(), 64);
    }

    #[test]
    fn same_label_produces_same_signature() {
        let (signing_key, _) = test_keypair();
        let label = UnsignedLabel {
            ver: 1,
            src: "did:plc:test".to_string(),
            uri: "at://did:plc:u/app.bsky.feed.post/x".to_string(),
            cid: None,
            val: "porn".to_string(),
            neg: false,
            cts: "2026-03-20T00:00:00Z".to_string(),
        };
        let sig1 = sign_label(&label, &signing_key).unwrap();
        let sig2 = sign_label(&label, &signing_key).unwrap();
        assert_eq!(sig1, sig2);
    }

    #[test]
    fn different_labels_produce_different_signatures() {
        let (signing_key, _) = test_keypair();
        let label1 = UnsignedLabel {
            ver: 1,
            src: "did:plc:test".to_string(),
            uri: "at://did:plc:u/app.bsky.feed.post/x".to_string(),
            cid: None,
            val: "nudity".to_string(),
            neg: false,
            cts: "2026-03-20T00:00:00Z".to_string(),
        };
        let label2 = UnsignedLabel {
            val: "porn".to_string(),
            ..label1.clone()
        };
        let sig1 = sign_label(&label1, &signing_key).unwrap();
        let sig2 = sign_label(&label2, &signing_key).unwrap();
        assert_ne!(sig1, sig2);
    }

    #[test]
    fn signing_key_from_hex_works() {
        let hex_key = "ab".repeat(32);
        let key = signing_key_from_hex(&hex_key).unwrap();
        assert_eq!(key.to_bytes().len(), 32);
    }
}
