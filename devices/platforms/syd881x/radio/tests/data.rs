//! Fragment construction for a standard HCI Controller adapter.
#![allow(clippy::unwrap_used, reason = "Unexpected test setup failure")]
use syd881x_radio::DataPdu;

#[test]
fn test_should_encode_owned_hci_fragments_without_repeating_l2cap_headers() {
    let start = DataPdu::fragment(2, &[1, 0, 4, 0, 0x0a]).unwrap();
    assert_eq!(start.as_slice(), &[2, 5, 1, 0, 4, 0, 0x0a]);
    assert_eq!(
        DataPdu::fragment(1, &[1, 2]).unwrap().as_slice(),
        &[1, 2, 1, 2]
    );
    assert_eq!(DataPdu::fragment(1, &[]).unwrap().as_slice(), &[1, 0]);
    for (id, data) in [
        (0, &[0; 4][..]),
        (4, &[0; 4][..]),
        (2, &[0; 3][..]),
        (3, &[][..]),
        (1, &[0; 28][..]),
    ] {
        assert!(DataPdu::fragment(id, data).is_err());
    }
}
