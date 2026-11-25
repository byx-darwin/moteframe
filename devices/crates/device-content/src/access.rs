use crate::Error;
/// Trusted authenticated owner/connection scope, supplied by integration, never packet fields.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Scope {
    owner: u64,
    connection: u64,
}
impl Scope {
    /// Construct an authenticated scope from the session layer.
    /// # Errors
    /// Rejects zero identities.
    pub fn new(owner: u64, connection: u64) -> Result<Self, Error> {
        if owner == 0 || connection == 0 {
            return Err(Error::Invalid);
        }
        Ok(Self { owner, connection })
    }
    /// Owner identity for a trusted policy port.
    #[must_use]
    pub const fn owner(self) -> u64 {
        self.owner
    }
    /// Connection incarnation; must change on reconnection.
    #[must_use]
    pub const fn connection(self) -> u64 {
        self.connection
    }
}
/// Policy operation checked by a trusted source-phone access port.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    /// Submit a device-originated action.
    Submit,
    /// Accept a result from the authenticated source phone.
    Settle,
    /// Use a phone-owned notification.
    Notification,
    /// Start or stop a find operation.
    Find,
}
/// Integration-provided source-phone authority. No packet can authorize itself.
pub trait AccessPort: core::fmt::Debug {
    /// Verify real authenticated scope and operation.
    /// # Errors
    /// Returns denial or unavailable authentication backend.
    fn authorize(&self, scope: Scope, operation: Operation) -> Result<(), Error>;
}

impl core::fmt::Debug for Scope {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Scope")
            .field("length", &16)
            .finish_non_exhaustive()
    }
}
