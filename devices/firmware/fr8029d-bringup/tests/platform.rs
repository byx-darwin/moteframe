//! Host tests for snapshot checks and packet RAM boundaries.

use fr8029d_bringup::{
    DescriptorError, DescriptorKind, DescriptorRegion, ProbeResult, Snapshot, XIP_ENTRY,
};

const BOARD: Snapshot = Snapshot {
    cpuid: 0x410f_d210,
    vtor: 0x1000_2000,
    qspi_config: 0x8089_0087,
    qspi_size: 0x0010_1002,
    qspi_remap: 0,
};

#[test]
fn test_should_recognize_observed_xip_configuration() {
    assert_eq!(BOARD.classify(), ProbeResult::Ready);
    assert_eq!(BOARD.qspi_address_bytes(), 3);
    assert_eq!(XIP_ENTRY, 0x1000_2000);
}

#[test]
fn test_should_report_wrong_core_or_boot_vector_before_qspi_configuration() {
    let other = Snapshot {
        cpuid: 0x410c_c200,
        vtor: 0,
        qspi_config: 0,
        ..BOARD
    };
    assert_eq!(other.classify(), ProbeResult::UnsupportedCore);
    let other = Snapshot {
        vtor: 0x1000_4000,
        qspi_config: 0,
        ..BOARD
    };
    assert_eq!(other.classify(), ProbeResult::UnexpectedVectorTable);
}

#[test]
fn test_should_require_direct_access_without_address_remap() {
    for invalid in [
        Snapshot {
            qspi_config: BOARD.qspi_config & !(1 << 7),
            ..BOARD
        },
        Snapshot {
            qspi_config: BOARD.qspi_config | (1 << 14),
            ..BOARD
        },
        Snapshot {
            qspi_remap: 0x2000,
            ..BOARD
        },
        Snapshot {
            qspi_size: 1,
            ..BOARD
        },
    ] {
        assert_eq!(invalid.classify(), ProbeResult::UnexpectedXipConfiguration);
    }
}

#[test]
fn test_should_resolve_rx_and_connection_spans_in_packet_ram() -> Result<(), DescriptorError> {
    let rx = DescriptorRegion::new(DescriptorKind::Rx, 0x600, 8)?;
    assert_eq!(rx.address(7), Ok(0x4001_06fc));
    assert_eq!(rx.address(8), Err(DescriptorError::IndexOutOfRange));
    let control = DescriptorRegion::new(DescriptorKind::ConnectionControl, 0x150, 2)?;
    assert_eq!(control.address(1), Ok(0x4001_01e4));
    Ok(())
}

#[test]
fn test_should_reject_alignment_overflow_and_partial_last_descriptor() -> Result<(), DescriptorError>
{
    for (offset, count, result) in [
        (0x2, 1, DescriptorError::Misaligned),
        (0x4000, 1, DescriptorError::OutOfRange),
        (0x3fe0, 1, DescriptorError::OutOfRange),
        (0xffff_fffc, 1, DescriptorError::OutOfRange),
        (0, u32::MAX, DescriptorError::OutOfRange),
        (0, 0, DescriptorError::Empty),
    ] {
        assert_eq!(
            DescriptorRegion::new(DescriptorKind::Rx, offset, count),
            Err(result)
        );
    }
    let last = DescriptorRegion::new(DescriptorKind::Rx, 0x3fdc, 1)?;
    assert_eq!(last.address(0), Ok(0x4001_3fdc));
    Ok(())
}
