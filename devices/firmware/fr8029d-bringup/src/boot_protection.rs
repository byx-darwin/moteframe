//! Restore the P25Q80 volatile protection baseline after ROM installation.
pub(super) fn ensure(mut command: impl FnMut(u32, u32) -> u32) -> bool {
    const SR1: u32 = 0x0580_0000;
    const SR2: u32 = 0x3580_0000;
    const LOCKED: u32 = 0x7c;

    if command(0x9fa0_0000, 0) & 0xff_ffff != 0x14_6085 || command(SR2, 0) & 255 != 2 {
        return false;
    }
    match command(SR1, 0) & 255 {
        LOCKED => true,
        0 => {
            // Volatile status-write enable followed by a single SR1 byte. Preserve QE in SR2.
            command(0x5000_0000, 0);
            command(0x0100_8000, LOCKED);
            command(SR1, 0) & 255 == LOCKED && command(SR2, 0) & 255 == 2
        }
        _ => false,
    }
}
