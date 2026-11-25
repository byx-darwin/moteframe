use crate::{AccessPort, Error, Operation, Scope};
/// Optional action, with feature-specific payload dictionaries.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Device asks phone to take one picture.
    #[cfg(feature = "media")]
    CameraShutter,
    /// Device requests phone music action 1..5.
    #[cfg(feature = "media")]
    PhoneMusic(u8),
    /// Phone controls selected device player, action 1..5.
    #[cfg(feature = "media")]
    DeviceMusic(u8),
    /// Desired local camera mode; no photograph is implied.
    #[cfg(feature = "media")]
    CameraMode(bool),
    /// SOS candidate source 1..3 with optional UTC.
    #[cfg(feature = "phone")]
    Sos {
        /// Source key/UI/automatic.
        source: u8,
        /// UTC milliseconds.
        utc_ms: i64,
        /// Validity, false requires UTC zero.
        time_valid: bool,
    },
    /// Request current P0002 from source phone.
    #[cfg(feature = "phone")]
    PhoneStatus,
}
/// Explicit backend submission outcome, separate from phone result acknowledgement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Submission {
    /// Actually submitted, awaiting a phone/platform result.
    AwaitingResult,
    /// Backend confirmed local completion.
    Completed,
}
/// Product-selected real action backend. Test implementations stay in tests.
pub trait ActionPort: core::fmt::Debug {
    /// Whether this real backend supports the selected action.
    fn available(&self, action: Action) -> bool;
    /// Perform one admitted action; never called again for the same ID.
    /// # Errors
    /// Returns an actual backend failure, never fabricated success.
    fn execute(&mut self, scope: Scope, id: u32, action: Action) -> Result<Submission, Error>;
}
/// Lifecycle of an action, without asserting cross-connection exactly-once execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionState {
    /// Reserved; no backend side effect.
    Prepared,
    /// Backend accepted, actual phone result not yet known.
    AwaitingResult,
    /// Immutable terminal wire result.
    Settled(u8),
    /// Real local backend confirmed completion.
    Completed,
    /// Backend returned a failure.
    Failed,
    /// Scope ended before a result was confirmed.
    Unknown,
}
/// Bounded connection-scoped action ledger.
pub struct ActionService<const N: usize> {
    scope: Scope,
    next: u32,
    entries: [Option<ActionEntry>; N],
}
#[derive(Debug, Clone, Copy)]
struct ActionEntry {
    id: u32,
    action: Action,
    state: ActionState,
}
impl<const N: usize> ActionService<N> {
    /// Create an empty ledger for one authenticated scope.
    #[must_use]
    pub const fn new(scope: Scope) -> Self {
        Self {
            scope,
            next: 1,
            entries: [None; N],
        }
    }
    /// Reserve a new nonzero event identity without invoking the backend.
    /// # Errors
    /// Returns invalid action, denial, missing backend, or capacity exhaustion.
    pub fn prepare(
        &mut self,
        action: Action,
        access: &impl AccessPort,
        port: &impl ActionPort,
    ) -> Result<u32, Error> {
        validate_action(action)?;
        access.authorize(self.scope, Operation::Submit)?;
        if !port.available(action) {
            return Err(Error::Unavailable);
        }
        #[cfg(feature = "media")]
        if let Action::CameraMode(enabled) = action
            && let Some(e) = self
                .entries
                .iter()
                .flatten()
                .rev()
                .find(|e| matches!(e.action, Action::CameraMode(_)))
            && e.action == Action::CameraMode(enabled)
            && !matches!(e.state, ActionState::Failed | ActionState::Unknown)
        {
            return Ok(e.id);
        }
        #[cfg(feature = "phone")]
        if action == Action::PhoneStatus
            && let Some(e) = self.entries.iter().flatten().find(|e| {
                e.action == Action::PhoneStatus
                    && matches!(e.state, ActionState::Prepared | ActionState::AwaitingResult)
            })
        {
            return Ok(e.id);
        }
        let slot = self
            .entries
            .iter_mut()
            .find(|e| e.is_none())
            .ok_or(Error::Capacity)?;
        let id = self.next;
        let next = id.checked_add(1).ok_or(Error::Capacity)?;
        *slot = Some(ActionEntry {
            id,
            action,
            state: ActionState::Prepared,
        });
        self.next = next;
        Ok(id)
    }
    /// Invoke a backend only after the complete successful response/event entered the transport queue.
    /// # Errors
    /// Returns denial, unknown ID, missing backend, or actual execution error.
    pub fn response_admitted(
        &mut self,
        id: u32,
        access: &impl AccessPort,
        port: &mut impl ActionPort,
    ) -> Result<ActionState, Error> {
        access.authorize(self.scope, Operation::Submit)?;
        let scope = self.scope;
        let e = self.entry_mut(id)?;
        if e.state != ActionState::Prepared {
            return Ok(e.state);
        }
        if !port.available(e.action) {
            return Err(Error::Unavailable);
        }
        e.state = ActionState::AwaitingResult;
        match port.execute(scope, id, e.action) {
            Ok(Submission::AwaitingResult) => Ok(e.state),
            Ok(Submission::Completed) => {
                e.state = ActionState::Completed;
                Ok(e.state)
            }
            Err(error) => {
                e.state = ActionState::Failed;
                Err(error)
            }
        }
    }
    /// Settle a source-phone result; camera/music use 0..3 and SOS uses 1..4.
    /// # Errors
    /// Unknown ID is `NotFound`; conflicting result is `Conflict`; unadmitted actions are `State`.
    pub fn settle(&mut self, id: u32, result: u8, access: &impl AccessPort) -> Result<(), Error> {
        access.authorize(self.scope, Operation::Settle)?;
        let e = self.entry_mut(id)?;
        validate_result(e.action, result)?;
        match e.state {
            ActionState::AwaitingResult => {
                e.state = ActionState::Settled(result);
                Ok(())
            }
            ActionState::Settled(old) if old == result => Ok(()),
            ActionState::Settled(_)
            | ActionState::Completed
            | ActionState::Failed
            | ActionState::Unknown => Err(Error::Conflict),
            ActionState::Prepared => Err(Error::State),
        }
    }
    /// Query current state without interpreting an acknowledgement as a photograph or call.
    /// # Errors
    /// Returns `NotFound` for an unknown ID.
    pub fn state(&self, id: u32) -> Result<ActionState, Error> {
        self.entries
            .iter()
            .flatten()
            .find(|e| e.id == id)
            .map(|e| e.state)
            .ok_or(Error::NotFound)
    }
    /// Serialize an outgoing phone event. Camera/music are 5B, SOS is 14B, status request is empty.
    /// # Errors
    /// Returns unknown ID, local-only action, or insufficient buffer.
    pub fn event_bytes(&self, id: u32, out: &mut [u8]) -> Result<usize, Error> {
        let e = self
            .entries
            .iter()
            .flatten()
            .find(|e| e.id == id)
            .ok_or(Error::NotFound)?;
        if e.state != ActionState::Prepared {
            return Err(Error::State);
        }
        let mut w = crate::wire::Writer::new(out);
        match e.action {
            #[cfg(feature = "media")]
            Action::CameraShutter => {
                w.u32(id)?;
                w.u8(1)?;
            }
            #[cfg(feature = "media")]
            Action::PhoneMusic(action) => {
                w.u32(id)?;
                w.u8(action)?;
            }
            #[cfg(feature = "media")]
            Action::DeviceMusic(_) | Action::CameraMode(_) => return Err(Error::State),
            #[cfg(feature = "phone")]
            Action::Sos {
                source,
                utc_ms,
                time_valid,
            } => {
                w.u32(id)?;
                w.u8(source)?;
                w.bytes(&utc_ms.to_le_bytes())?;
                w.u8(u8::from(time_valid))?;
            }
            #[cfg(feature = "phone")]
            Action::PhoneStatus => {}
        }
        Ok(w.len())
    }
    /// Complete a coalesced status request only after the source phone actually pushes P0002.
    /// # Errors
    /// Returns unknown identity, denied access, wrong action or invalid call order.
    #[cfg(feature = "phone")]
    pub fn status_received(&mut self, id: u32, access: &impl AccessPort) -> Result<(), Error> {
        access.authorize(self.scope, Operation::Settle)?;
        let entry = self.entry_mut(id)?;
        if entry.action != Action::PhoneStatus {
            return Err(Error::State);
        }
        match entry.state {
            ActionState::AwaitingResult => {
                entry.state = ActionState::Completed;
                Ok(())
            }
            ActionState::Completed => Ok(()),
            _ => Err(Error::State),
        }
    }
    /// Cancel pending work when scope, authorization or CCCD is lost. Unconfirmed outcomes are Unknown.
    #[must_use]
    pub fn invalidate(&mut self) -> UnknownActions<N> {
        let mut ids = [None; N];
        let mut count = 0;
        for e in self.entries.iter_mut().flatten() {
            if matches!(e.state, ActionState::Prepared | ActionState::AwaitingResult) {
                e.state = ActionState::Unknown;
                ids[count] = Some(e.id);
                count += 1;
            }
        }
        UnknownActions { ids, count }
    }
    /// Replace a connection scope after publishing Unknown outcomes; old IDs/results are forgotten.
    /// # Errors
    /// Rejects accidental reset of the same authenticated connection incarnation.
    pub fn replace_scope(&mut self, scope: Scope) -> Result<UnknownActions<N>, Error> {
        if scope == self.scope {
            return Err(Error::Conflict);
        }
        let unknown = self.invalidate();
        self.entries = [None; N];
        self.next = 1;
        self.scope = scope;
        Ok(unknown)
    }
    fn entry_mut(&mut self, id: u32) -> Result<&mut ActionEntry, Error> {
        self.entries
            .iter_mut()
            .flatten()
            .find(|e| e.id == id)
            .ok_or(Error::NotFound)
    }
}
impl<const N: usize> core::fmt::Debug for ActionService<N> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ActionService")
            .field("length", &self.entries.iter().flatten().count())
            .finish_non_exhaustive()
    }
}
/// IDs whose outcome is unknown after cancellation; integration must not replay them.
#[derive(Clone, Copy)]
pub struct UnknownActions<const N: usize> {
    ids: [Option<u32>; N],
    count: usize,
}
impl<const N: usize> UnknownActions<N> {
    /// Number of unconfirmed actions.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.count
    }
    /// Whether every action was already terminal.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.count == 0
    }
    /// Read one unknown ID.
    /// # Errors
    /// Returns `NotFound` for an invalid index.
    pub fn id(&self, index: usize) -> Result<u32, Error> {
        self.ids
            .get(index)
            .copied()
            .flatten()
            .ok_or(Error::NotFound)
    }
}
impl<const N: usize> core::fmt::Debug for UnknownActions<N> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("UnknownActions")
            .field("length", &self.count)
            .finish_non_exhaustive()
    }
}
fn validate_action(action: Action) -> Result<(), Error> {
    match action {
        #[cfg(feature = "media")]
        Action::PhoneMusic(a) | Action::DeviceMusic(a) if !(1..=5).contains(&a) => {
            Err(Error::Invalid)
        }
        #[cfg(feature = "phone")]
        Action::Sos {
            source,
            utc_ms,
            time_valid,
        } if !(1..=3).contains(&source) || (!time_valid && utc_ms != 0) => Err(Error::Invalid),
        _ => Ok(()),
    }
}
fn validate_result(action: Action, result: u8) -> Result<(), Error> {
    let valid = match action {
        #[cfg(feature = "media")]
        Action::CameraShutter | Action::PhoneMusic(_) => result <= 3,
        #[cfg(feature = "media")]
        Action::DeviceMusic(_) | Action::CameraMode(_) => false,
        #[cfg(feature = "phone")]
        Action::Sos { .. } => (1..=4).contains(&result),
        #[cfg(feature = "phone")]
        Action::PhoneStatus => false,
    };
    if valid { Ok(()) } else { Err(Error::Invalid) }
}
/// Media action state machine; all event identities share its monotonic ledger.
#[cfg(feature = "media")]
pub type MediaService<const N: usize> = ActionService<N>;
/// Phone/SOS action state machine; actual action support comes from the port.
#[cfg(feature = "phone")]
pub type PhoneService<const N: usize> = ActionService<N>;

impl core::fmt::Debug for Action {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Action")
            .field("length", &core::mem::size_of_val(self))
            .finish_non_exhaustive()
    }
}
