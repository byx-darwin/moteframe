//! Offline monomorphization for actual ARM frame/type accounting.
//! This slice reader is a compiler probe only, never a hardware substitute.
#![no_std]
#![forbid(unsafe_code)]
use device_core::ReadAt;
use device_resource_archive::{Archive,ArchiveError};
#[derive(Debug)]
struct Slice<'a>(&'a [u8]);
impl ReadAt for Slice<'_> {
    type Error=();
    fn capacity(&self)->u32 {u32::try_from(self.0.len()).unwrap_or(u32::MAX)}
    fn read_exact(&mut self,offset:u32,out:&mut[u8])->Result<(),()> {
        if out.len()>256 {return Err(());}
        let start=usize::try_from(offset).map_err(|_|())?;let end=start.checked_add(out.len()).ok_or(())?;
        out.copy_from_slice(self.0.get(start..end).ok_or(())?);Ok(())
    }
}
/// Real archive construction and decoder monomorphized for Thumbv6-M.
/// # Errors
/// Returns the unmodified archive/read failure.
#[inline(never)]
pub fn read(bytes:&[u8],expected:[u8;32],at:u32,out:&mut[u8])->Result<(),ArchiveError<()>> {
    Archive::open(Slice(bytes),expected)?.read_exact(at,out)
}
/// Concrete ARM archive handle size, alignment and fixed block bytes.
pub const TYPE_BUDGET:[usize;3]=[core::mem::size_of::<Archive<Slice<'static>>>(),core::mem::align_of::<Archive<Slice<'static>>>(),device_resource_archive::BLOCK_BYTES];
/// Exposes the actual type budget in emitted LLVM IR.
#[inline(never)]
pub fn type_budget()->[usize;3] {TYPE_BUDGET}
