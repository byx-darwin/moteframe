//! Checked W5BOOT01 payload policy. Persistence is performed by the actual boot engine.
use device_storage::persistence::Snapshot;
/// Exact journal binding or monotonic-counter failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// Wrong magic, length, identities, reserved bytes or counter/revision binding.
    Invalid,
    /// Counter or revision cannot advance without reuse.
    Exhausted,
}
/// Build the next complete journal payload without media I/O or publishing an identity.
/// Caller may use the counter only after actual IncrementalSnapshot durable readback.
/// # Errors
/// Rejects unknown format, foreign provisioning/namespace, noncanonical reserved
/// bytes, inconsistent revision or exhaustion. Never defaults or reformats.
pub fn advance(
    snapshot: Snapshot,
    bytes: &[u8; 64],
    provision: [u8; 16],
    namespace: [u8; 16],
) -> Result<([u8; 64], u64), Error> {
    if snapshot.length != 64
        || provision == [0; 16]
        || namespace == [0; 16]
        || &bytes[..8] != b"W5BOOT01"
        || bytes[8..24] != provision
        || bytes[24..40] != namespace
        || bytes[48..].iter().any(|b| *b != 0)
    {
        return Err(Error::Invalid);
    }
    let count = u64::from_le_bytes(bytes[40..48].try_into().map_err(|_| Error::Invalid)?);
    if count.checked_add(1).ok_or(Error::Exhausted)? != snapshot.revision {
        return Err(Error::Invalid);
    }
    let next = count.checked_add(1).ok_or(Error::Exhausted)?;
    snapshot.revision.checked_add(1).ok_or(Error::Exhausted)?;
    let mut out = *bytes;
    out[40..48].copy_from_slice(&next.to_le_bytes());
    Ok((out, next))
}
