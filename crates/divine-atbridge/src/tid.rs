//! ATProto TID generation for durable, caller-supplied record keys.

use secp256k1::rand::{rngs::OsRng, RngCore};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

const SORTABLE_BASE32: &[u8; 32] = b"234567abcdefghijklmnopqrstuvwxyz";
const MAX_TID_TIMESTAMP: u64 = (1_u64 << 53) - 1;

static LAST_TIMESTAMP_MICROS: AtomicU64 = AtomicU64::new(0);
static CLOCK_ID: OnceLock<u16> = OnceLock::new();

/// Generate a new ATProto TID.
pub fn next_tid() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock must be after the Unix epoch")
        .as_micros() as u64;
    let timestamp = reserve_monotonic_timestamp(now);
    assert!(
        timestamp <= MAX_TID_TIMESTAMP,
        "microsecond timestamp exceeds the ATProto TID range"
    );

    let clock_id = *CLOCK_ID.get_or_init(|| (OsRng.next_u32() & 0x03ff) as u16);
    let mut tid = String::with_capacity(13);
    encode_fixed_base32(timestamp, 11, &mut tid);
    encode_fixed_base32(u64::from(clock_id), 2, &mut tid);
    tid
}

fn reserve_monotonic_timestamp(now: u64) -> u64 {
    let mut observed = LAST_TIMESTAMP_MICROS.load(Ordering::Relaxed);
    loop {
        let next = now.max(observed.saturating_add(1));
        match LAST_TIMESTAMP_MICROS.compare_exchange_weak(
            observed,
            next,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => return next,
            Err(actual) => observed = actual,
        }
    }
}

fn encode_fixed_base32(mut value: u64, width: usize, output: &mut String) {
    let mut encoded = vec![b'2'; width];
    for index in (0..width).rev() {
        encoded[index] = SORTABLE_BASE32[(value & 0x1f) as usize];
        value >>= 5;
    }
    assert_eq!(value, 0, "value does not fit in fixed-width base32");
    output.push_str(std::str::from_utf8(&encoded).expect("TID alphabet must be valid UTF-8"));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tids_have_the_standard_shape() {
        let tid = next_tid();
        assert_eq!(tid.len(), 13);
        assert!(tid.bytes().all(|c| SORTABLE_BASE32.contains(&c)));
    }

    #[test]
    fn tids_are_strictly_monotonic() {
        let mut previous = next_tid();
        for _ in 0..10_000 {
            let current = next_tid();
            assert!(current > previous, "{current} must sort after {previous}");
            previous = current;
        }
    }
}
