//! Standard parameter validation before a chip backend is touched.
#![allow(
    clippy::unwrap_used,
    reason = "Unexpected setup failure aborts the test"
)]
use ble_controller::{AdvertisingData, AdvertisingParameters, ConnectionParameters, RandomAddress};

#[test]
fn test_should_validate_standard_parameters_without_chip_register_units() {
    assert!(ConnectionParameters::new(24, 0, 72).is_ok());
    for values in [
        (5, 0, 72),
        (3201, 0, 72),
        (24, 500, 72),
        (24, 0, 9),
        (40, 0, 10),
    ] {
        assert!(ConnectionParameters::new(values.0, values.1, values.2).is_err());
    }
    assert!(ConnectionParameters::new(24, 1, 72).is_ok());
    assert!(AdvertisingParameters::new(160, 160, 1).is_ok());
    for (a, b, c) in [
        (31, 160, 1),
        (160, 32, 1),
        (160, 16385, 1),
        (160, 160, 0),
        (160, 160, 8),
    ] {
        assert!(AdvertisingParameters::new(a, b, c).is_err());
    }
}

#[test]
fn test_should_bound_owned_advertising_data_and_static_random_identity() {
    let mut bytes = [2, 1, 6];
    let data = AdvertisingData::new(&bytes).unwrap();
    bytes[2] = 0;
    assert_eq!(bytes[2], 0);
    assert_eq!(data.as_slice(), &[2, 1, 6]);
    assert!(AdvertisingData::new(&[0; 32]).is_err());
    assert!(RandomAddress::new([1, 2, 3, 4, 5, 0xc0]).is_ok());
    for bytes in [
        [0; 6],
        [0xff; 6],
        [0, 0, 0, 0, 0, 0xc0],
        [1, 2, 3, 4, 5, 0x80],
    ] {
        assert!(RandomAddress::new(bytes).is_err());
    }
}
