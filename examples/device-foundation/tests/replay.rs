//! A seeded whole-composition workload must exercise recovery and all service lanes.
#![allow(clippy::unwrap_used)]
use moteframe_device_foundation::{model::Sink, probe};
#[test]
fn seeded_workload_reaches_each_lane_and_recovers_expected_faults() {
    let mut hashes = Vec::new();
    for seed in [0, 1, 2, 3, 4, 8, 65_535, u32::MAX] {
        let mut sink = Sink::new();
        let o = probe::exercise(&mut sink, seed).unwrap();
        assert_eq!(o.samples, 16);
        assert_eq!(o.events, 16);
        assert_eq!(o.accepted, 16);
        assert_eq!(o.received, 16);
        assert_eq!(o.recoveries, 3);
        assert_eq!(o.selection, 0);
        assert!(sink.pixels >= 512);
        hashes.push(o.tx_fingerprint);
    }
    assert!(hashes.windows(2).any(|p| p[0] != p[1]));
}
