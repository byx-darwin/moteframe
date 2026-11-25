use crate::Error;
/// Nonzero operation ticket allocated by the real selected backend, never guessed by a service.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct BackendTicket(core::num::NonZeroU32);
impl BackendTicket {
    /// Wrap a real backend ticket.
    /// # Errors
    /// Rejects zero.
    pub fn new(value: u32) -> Result<Self, Error> {
        core::num::NonZeroU32::new(value)
            .map(Self)
            .ok_or(Error::Invalid)
    }
    /// Underlying backend identity, for the actual adapter only.
    #[must_use]
    pub const fn value(self) -> u32 {
        self.0.get()
    }
}
impl core::fmt::Debug for BackendTicket {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("BackendTicket")
            .field("length", &4)
            .finish_non_exhaustive()
    }
}
