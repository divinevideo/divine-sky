// CID construction and blob utilities
// Converts between Blossom SHA-256 hex hashes and ATProto CID strings

use cid::Cid;
use multihash::Multihash;

/// SHA-256 multihash code
const SHA2_256: u64 = 0x12;
/// Raw codec for CIDv1
const RAW_CODEC: u64 = 0x55;

/// Convert a SHA-256 hex string to a CIDv1 string (base32lower, raw codec).
///
/// This bridges Blossom/Nostr SHA-256 hashes to ATProto CID format.
pub fn sha256_to_cid(sha256_hex: &str) -> anyhow::Result<String> {
    let digest_bytes = hex::decode(sha256_hex)?;
    anyhow::ensure!(digest_bytes.len() == 32, "SHA-256 must be 32 bytes");

    let mh = Multihash::<64>::wrap(SHA2_256, &digest_bytes)?;
    let cid = Cid::new_v1(RAW_CODEC, mh);
    Ok(cid.to_string())
}

/// Extract the SHA-256 hex digest from a CID string.
pub fn cid_to_sha256(cid_str: &str) -> anyhow::Result<String> {
    let cid: Cid = cid_str.try_into()?;
    let mh = cid.hash();
    let digest = mh.digest();
    Ok(hex::encode(digest))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sha256_hex_to_cid() {
        // Known SHA-256 hash (sha256 of "hello")
        let sha256_hex = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";
        let cid = sha256_to_cid(sha256_hex).unwrap();

        // CID should start with "bafkrei" (base32lower CIDv1 raw)
        assert!(
            cid.starts_with("bafkrei"),
            "CID should start with 'bafkrei', got: {}",
            cid
        );

        // Round-trip: extract hash from CID, compare
        let extracted = cid_to_sha256(&cid).unwrap();
        assert_eq!(extracted, sha256_hex);
    }

    #[test]
    fn test_invalid_hex() {
        let result = sha256_to_cid("not-valid-hex");
        assert!(result.is_err());
    }

    #[test]
    fn test_wrong_length() {
        // Valid hex but not 32 bytes
        let result = sha256_to_cid("abcd");
        assert!(result.is_err());
    }

    #[test]
    fn test_empty_string() {
        let result = sha256_to_cid("");
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_cid_string() {
        let result = cid_to_sha256("not-a-valid-cid");
        assert!(result.is_err());
    }
}
