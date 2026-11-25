//! No physical writable binding is supplied by the observed density or this plan.
//!
//! The legacy CFG range is actively used by the original C firmware. These
//! constants describe a proposed future new-product provisioning format; they
//! neither reassign that range nor authorize formatting, writes or migration.

/// Current board storage availability. There is deliberately no ready variant.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WritableStorageBinding {
    /// Exact part/protection/timing/durability and exclusive provisioning are absent.
    Unavailable,
}
/// Report actual evidence state without touching the device or constructing a writer.
#[must_use]
pub const fn writable_storage_binding() -> WritableStorageBinding {
    WritableStorageBinding::Unavailable
}
/// Observed JEDEC bytes only; no exact part identification or write qualification.
pub const OBSERVED_EXTERNAL_JEDEC: [u8; 3] = [0x5e, 0x40, 0x16];
/// Conventional density interpretation, not an independent silicon capacity test.
pub const OBSERVED_EXTERNAL_DENSITY: u32 = 0x40_0000;
/// Original complete captured4MiB image; source is an existing offline backup.
pub const ORIGINAL_EXTERNAL_SHA256: &str =
    "cc7533616efdff0ebe7acd275ae5ec89d3ff890bb21ec01409c395b69b47dc29";

/// A complete range in the offline proposal, with its original captured hash.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProposedRange {
    /// Physical start, inclusive.
    pub start: u32,
    /// Physical end, exclusive.
    pub end: u32,
    /// SHA256 of original bytes; never an expected hash of formatted new storage.
    pub original_sha256: &'static str,
}
/// Entire legacy16KiB CFG: proposed two8KiB settings banks, still unprovisioned.
pub const PROPOSED_SETTINGS_BANKS: [ProposedRange; 2] = [
    ProposedRange {
        start: 0x1a_3000,
        end: 0x1a_5000,
        original_sha256: "43e7b36c116c91570993bb836b074f34e76a610deca54e044f07a42955fa1c74",
    },
    ProposedRange {
        start: 0x1a_5000,
        end: 0x1a_7000,
        original_sha256: "e29804f89d188787fc8c011721fdd38bc7886a12728ba030417636aad4e574b4",
    },
];
/// All other original bytes: fonts, legacy DATA, logo/applet/UI; no write grant.
pub const PRESERVED_EXTERNAL_RANGES: [ProposedRange; 3] = [
    ProposedRange {
        start: 0,
        end: 0x1a_3000,
        original_sha256: "42a1de731358af9ac6b0ca2ab359d81c9b691ed6997737e9e6fc749b17045a93",
    },
    ProposedRange {
        start: 0x1a_7000,
        end: 0x1b_f000,
        original_sha256: "4ed66ad97b23d556ecc94fd45fdcdf07f6b9d88445424304ccaf4f8bc1017fc3",
    },
    ProposedRange {
        start: 0x1b_f000,
        end: 0x40_0000,
        original_sha256: "3651c2cf0ab721d6c81606de3f509c3ddac4bb99d90b954571f52e7ef214f872",
    },
];
/// Proposed logical unit, giving a256-byte header reservation and256-byte marker.
/// Reopening existing storage must use its exact original geometry/layout.
pub const PROPOSED_SETTINGS_WRITE_UNIT: u32 = 256;
/// Selected settings image bound, before owner/engine/adapter RAM budgeting.
pub const PROPOSED_SETTINGS_PAYLOAD_BOUND: usize = 256;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_should_keep_unprovisioned_board_unavailable_and_plan_disjoint() {
        assert_eq!(
            writable_storage_binding(),
            WritableStorageBinding::Unavailable
        );
        let mut ranges = [
            PRESERVED_EXTERNAL_RANGES[0],
            PROPOSED_SETTINGS_BANKS[0],
            PROPOSED_SETTINGS_BANKS[1],
            PRESERVED_EXTERNAL_RANGES[1],
            PRESERVED_EXTERNAL_RANGES[2],
        ];
        ranges.sort_by_key(|range| range.start);
        let mut end = 0;
        for range in ranges {
            assert_eq!(range.start, end);
            assert!(range.end > range.start);
            assert!(range.start.is_multiple_of(4096) && range.end.is_multiple_of(4096));
            end = range.end;
        }
        assert_eq!(end, OBSERVED_EXTERNAL_DENSITY);
        for bank in PROPOSED_SETTINGS_BANKS {
            let payload = u32::try_from(PROPOSED_SETTINGS_PAYLOAD_BOUND)
                .unwrap_or_else(|error| panic!("payload bound: {error}"));
            assert!(PROPOSED_SETTINGS_WRITE_UNIT * 2 + payload <= bank.end - bank.start);
        }
    }
}
