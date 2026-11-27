//! Comparison adapter for the emulated, byte-addressable NOR window.
use moteframe_storage::write::{Slot, SlotRegion};
pub(super) struct Media;
fn address(s: Slot, i: usize) -> *mut u8 {
    (0x60000000 + 4096 * (s as usize) + i) as *mut u8
}
impl SlotRegion<256> for Media {
    type Error = ();
    fn slot_bytes(&self) -> usize {
        4096
    }
    fn ready(&mut self) -> Result<(), ()> {
        Ok(())
    }
    fn read(&mut self, s: Slot, o: usize, b: &mut [u8]) -> Result<(), ()> {
        for (i, x) in b.iter_mut().enumerate() {
            *x = unsafe { address(s, o + i).read_volatile() }
        }
        Ok(())
    }
    fn prepare(&mut self, s: Slot) -> Result<(), ()> {
        for i in 0..4096 {
            unsafe { address(s, i).write_volatile(255) }
        }
        Ok(())
    }
    fn program_page(&mut self, s: Slot, o: usize, b: &[u8; 256]) -> Result<(), ()> {
        for (i, x) in b.iter().enumerate() {
            unsafe { address(s, o + i).write_volatile(*x) }
        }
        Ok(())
    }
}
