//! Archive range, identity and independent block verification tests.
// Fixture conversions and assertions deliberately panic when a test fails.
#![allow(clippy::unwrap_used)]
use device_core::ReadAt;
use device_resource_archive::{Archive, ArchiveError, BLOCK_BYTES, VIRTUAL_BYTES};

#[derive(Debug)]
struct Bytes {
    data: Vec<u8>,
    fail: bool,
    max_read: usize,
}
impl ReadAt for Bytes {
    type Error = ();
    fn capacity(&self) -> u32 {
        u32::try_from(self.data.len()).unwrap()
    }
    fn read_exact(&mut self, at: u32, out: &mut [u8]) -> Result<(), ()> {
        self.max_read = self.max_read.max(out.len());
        if self.fail {
            return Err(());
        }
        out.copy_from_slice(
            self.data
                .get(at as usize..at as usize + out.len())
                .ok_or(())?,
        );
        Ok(())
    }
}
fn image() -> Bytes {
    let packed = [
        120, 218, 99, 96, 24, 5, 163, 96, 20, 140, 84, 0, 0, 4, 0, 0, 1,
    ];
    let end = 64 + 4096 * 12 + 4096 * packed.len();
    let mut data = vec![255; 0x0020_0000];
    data[..8].copy_from_slice(b"W5RARC01");
    data[8..12].copy_from_slice(&[1, 0, 1, 0]);
    for (at, value) in [
        (12, VIRTUAL_BYTES),
        (16, u32::try_from(BLOCK_BYTES).unwrap()),
        (20, 4096),
        (24, 49216),
        (28, u32::try_from(end).unwrap()),
    ] {
        data[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    data[32..64].fill(7);
    let mut pos = 49216;
    for n in 0..4096 {
        let at = 64 + n * 12;
        data[at..at + 4].copy_from_slice(&(u32::try_from(pos).unwrap()).to_le_bytes());
        data[at + 4..at + 6].copy_from_slice(&(u16::try_from(packed.len()).unwrap()).to_le_bytes());
        data[at + 6..at + 8].copy_from_slice(&1024_u16.to_le_bytes());
        data[at + 8..at + 12].copy_from_slice(&0xefb5_af2e_u32.to_le_bytes());
        data[pos..pos + packed.len()].copy_from_slice(&packed);
        pos += packed.len();
    }
    Bytes {
        data,
        fail: false,
        max_read: 0,
    }
}
#[test]
fn test_should_decode_cross_block_and_enforce_bounded_io() {
    let mut a = Archive::open(image(), [7; 32]).unwrap();
    let mut out = [9; 1100];
    a.read_exact(1000, &mut out).unwrap();
    assert_eq!(out, [0; 1100]);
    assert_eq!(a.capacity(), VIRTUAL_BYTES);
    assert!(a.into_inner().max_read <= 256);
}
#[test]
fn test_should_reject_bounds_identity_overlap_and_corruption() {
    assert!(matches!(
        Archive::open(image(), [8; 32]),
        Err(ArchiveError::Format)
    ));
    let mut i = image();
    i.data[76..80].copy_from_slice(&49216_u32.to_le_bytes());
    assert!(Archive::open(i, [7; 32]).is_err());
    let mut a = Archive::open(image(), [7; 32]).unwrap();
    assert!(matches!(
        a.read_exact(u32::MAX, &mut [0; 2]),
        Err(ArchiveError::Bounds)
    ));
    let mut i = image();
    i.data[49216 + 17] ^= 1;
    let mut a = Archive::open(i, [7; 32]).unwrap();
    assert!(a.read_exact(1024, &mut [0; 4]).is_err());
    let mut i = image();
    i.data[64 + 8] ^= 1;
    let mut a = Archive::open(i, [7; 32]).unwrap();
    assert!(matches!(
        a.read_exact(0, &mut [0; 4]),
        Err(ArchiveError::Integrity)
    ));
}
#[test]
fn test_should_propagate_media_failure() {
    let mut i = image();
    i.fail = true;
    assert!(matches!(
        Archive::open(i, [7; 32]),
        Err(ArchiveError::Source(()))
    ));
}

#[test]
fn test_should_preserve_busy_error_after_mount_and_check_exact_end_before_io() {
    #[derive(Debug)]
    struct FailAt {
        inner: Bytes,
        calls: usize,
        trigger: usize,
    }
    impl ReadAt for FailAt {
        type Error = ();
        fn capacity(&self) -> u32 {
            self.inner.capacity()
        }
        fn read_exact(&mut self, at: u32, out: &mut [u8]) -> Result<(), ()> {
            self.calls += 1;
            if self.calls == self.trigger {
                return Err(());
            }
            self.inner.read_exact(at, out)
        }
    }
    let mut archive = Archive::open(
        FailAt {
            inner: image(),
            calls: 0,
            trigger: 4099,
        },
        [7; 32],
    )
    .unwrap();
    assert!(matches!(
        archive.read_exact(0, &mut [0; 4]),
        Err(ArchiveError::Source(()))
    ));
    let mut archive = Archive::open(image(), [7; 32]).unwrap();
    assert!(archive.read_exact(VIRTUAL_BYTES, &mut []).is_ok());
    assert!(matches!(
        archive.read_exact(VIRTUAL_BYTES, &mut [0; 1]),
        Err(ArchiveError::Bounds)
    ));
    let mut last = [9];
    archive.read_exact(VIRTUAL_BYTES - 1, &mut last).unwrap();
    assert_eq!(last, [0]);
}
