//! Ordered register reads for complete raw touch frames, without controller policy.
#![no_std]
#![forbid(unsafe_code)]

/// Reads four independent one-byte registers in the supplied order.
/// Address and electrical validation belong to the reader.
/// # Errors
/// Stops on the first reader error and returns no partial frame.
pub fn read_frame<E>(
    address: u8,
    registers: [u8; 4],
    mut read: impl FnMut(u8, u8, &mut [u8]) -> Result<(), E>,
) -> Result<[u8; 4], E> {
    let mut frame = [0; 4];
    for (register, byte) in registers.into_iter().zip(&mut frame) {
        read(address, register, core::slice::from_mut(byte))?;
    }
    Ok(frame)
}
