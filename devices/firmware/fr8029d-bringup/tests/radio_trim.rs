//! Factory trim validation for cold radio startup.
use fr8029d_bringup::radio_trim::{RadioTrim, TRIM_BYTES, TrimError};

fn trim_record() -> [u8; TRIM_BYTES] {
    // Minimal synthetic version-2 record with all three RF calibrations.
    let mut bytes = [0; TRIM_BYTES];
    bytes[0] = 7;
    bytes[0x2f] = 5;
    bytes[0x3c] = 0x61;
    bytes[0x5c..0x5e].copy_from_slice(&0x0c34_u16.to_le_bytes());
    bytes[0x6c] = 0x1c;
    bytes[0x74] = 4;
    bytes[0x86] = 2;
    bytes[0x9c..].copy_from_slice(&0x9de4_24e7_u32.to_le_bytes());
    bytes
}

#[test]
fn reads_rf_fields_from_a_valid_version_two_record() {
    let Ok(trim) = RadioTrim::parse(&trim_record()) else {
        panic!("valid RF trim was rejected");
    };
    assert_eq!(trim.kvco, 5);
    assert_eq!(trim.rx_dc[0], 0x61);
    assert_eq!(trim.tx_modulation_scale, 0x0c34);
    assert_eq!(trim.tx_modulation_current, 0x1c);
    assert_eq!(trim.tx_power_steps[0], 4);
}

#[test]
fn rejects_erased_or_damaged_records() {
    assert_eq!(
        RadioTrim::parse(&[0xff; TRIM_BYTES]),
        Err(TrimError::Checksum)
    );
    let mut bytes = trim_record();
    bytes[0x5c] ^= 1;
    assert_eq!(RadioTrim::parse(&bytes), Err(TrimError::Checksum));
}
