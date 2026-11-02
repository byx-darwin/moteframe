//! Read-only views must not disclose source contents through Debug.
#![allow(clippy::unwrap_used)]
use moteframe_core::ReadAt;
use moteframe_storage::{ReadRef, ReadWindow, SliceSource};

struct PrivateSource;
impl ReadAt for PrivateSource {
    type Error = u8;
    fn capacity(&self) -> u32 {
        1
    }
    fn read_exact(&mut self, _: u32, _: &mut [u8]) -> Result<(), u8> {
        Err(17)
    }
}

#[test]
fn debug_does_not_require_or_call_backend_debug() {
    let mut source = PrivateSource;
    let reference = ReadRef::new(&mut source);
    assert!(format!("{reference:?}").starts_with("ReadRef"));
    let window = ReadWindow::new(reference, 0, 1).unwrap();
    assert!(format!("{window:?}").starts_with("ReadWindow"));
}

#[test]
fn debug_omits_source_bytes_even_through_nested_views() {
    let mut source = SliceSource::new(&[211, 212, 213]).unwrap();
    assert!(!format!("{source:?}").contains("211"));
    let reference = ReadRef::new(&mut source);
    assert!(!format!("{reference:?}").contains("211"));
    let window = ReadWindow::new(reference, 1, 2).unwrap();
    assert!(!format!("{window:?}").contains("211"));
}
