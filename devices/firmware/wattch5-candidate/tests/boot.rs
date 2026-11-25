//! Durable boot counters are parsed from the exact provisioned namespace.
#![allow(
    clippy::unwrap_used,
    reason = "Test assertions panic on invalid fixture."
)]
use device_storage::persistence::Snapshot;
use wattch5_candidate::boot::advance;
#[test]
fn test_should_increment_only_exact_journal_and_reject_wrong_identity_or_counter() {
    let provision = [7; 16];
    let namespace = [9; 16];
    let mut bytes = [0; 64];
    bytes[..8].copy_from_slice(b"W5BOOT01");
    bytes[8..24].copy_from_slice(&provision);
    bytes[24..40].copy_from_slice(&namespace);
    let snapshot = Snapshot {
        revision: 1,
        length: 64,
    };
    let (next, counter) = advance(snapshot, &bytes, provision, namespace).unwrap();
    assert_eq!(counter, 1);
    assert_eq!(&next[40..48], &1u64.to_le_bytes());
    assert_eq!(&bytes[40..48], &0u64.to_le_bytes());
    assert!(advance(snapshot, &bytes, [8; 16], namespace).is_err());
    assert!(
        advance(
            Snapshot {
                revision: 2,
                length: 64
            },
            &bytes,
            provision,
            namespace
        )
        .is_err()
    );
    bytes[63] = 1;
    assert!(advance(snapshot, &bytes, provision, namespace).is_err());
}
