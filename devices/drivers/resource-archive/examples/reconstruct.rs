//! Read-only host check of the real `no_std` decoder against a complete image.
use device_core::ReadAt;
use device_resource_archive::{Archive, BLOCK_BYTES, VIRTUAL_BYTES};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
};
#[derive(Debug)]
struct Physical {
    file: File,
    capacity: u32,
    max_read: usize,
}
impl ReadAt for Physical {
    type Error = std::io::Error;
    fn capacity(&self) -> u32 {
        self.capacity
    }
    fn read_exact(&mut self, at: u32, out: &mut [u8]) -> Result<(), Self::Error> {
        if out.len() > 256 {
            return Err(std::io::Error::other("read exceeds256bytes"));
        }
        self.max_read = self.max_read.max(out.len());
        self.file.seek(SeekFrom::Start(u64::from(at)))?;
        self.file.read_exact(out)
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 4 {
        return Err("usage: reconstruct IMAGE EXPECTED_ORIGINAL_SHA NEW_OUTPUT".into());
    }
    let mut expected = [0; 32];
    if args[2].len() != 64 {
        return Err("invalidSHA".into());
    }
    for (n, b) in expected.iter_mut().enumerate() {
        *b = u8::from_str_radix(&args[2][n * 2..n * 2 + 2], 16)?;
    }
    let file = File::open(&args[1])?;
    let capacity = u32::try_from(file.metadata()?.len())?;
    let mut archive = Archive::open(
        Physical {
            file,
            capacity,
            max_read: 0,
        },
        expected,
    )
    .map_err(|e| format!("archive: {e:?}"))?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&args[3])?;
    let mut block = [0; BLOCK_BYTES];
    for at in (0..VIRTUAL_BYTES).step_by(BLOCK_BYTES) {
        archive
            .read_exact(at, &mut block)
            .map_err(|e| format!("decode at{at:#x}: {e:?}"))?;
        output.write_all(&block)?;
    }
    println!(
        "decoded_bytes={} max_physical_read={} archive_size={}",
        VIRTUAL_BYTES,
        archive.into_inner().max_read,
        core::mem::size_of::<Archive<Physical>>()
    );
    Ok(())
}
