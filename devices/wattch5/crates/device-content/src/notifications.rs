//! Trusted source-phone notification lifecycle and action settlement.
use crate::{
    AccessPort, Error, Operation, Scope,
    wire::{Writer, ensure},
};
/// Shared schema2 notification, without a duplicate conflicting model.
pub use device_models::Notification;
/// Notification content change classification; retrying identical COMMIT never alerts twice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationChange {
    /// New notification identity; may alert after admission.
    New,
    /// Existing notification updated; no new alert.
    Updated,
    /// Exact COMMIT retry.
    Unchanged,
}
/// Actual notification UI/source-phone backend.
pub trait NotificationPort: core::fmt::Debug {
    /// Real support for call action bits 0 answer, 1 reject, 2 mute.
    /// # Errors
    /// Returns `Unavailable` when no real backend is selected.
    fn supported_actions(&self, scope: Scope) -> Result<u16, Error>;
    /// Present admitted content; alert only for a new notification identity.
    /// # Errors
    /// Returns actual display/backend failure.
    fn present(
        &mut self,
        scope: Scope,
        object_id: u32,
        value: Notification<'_>,
        alert: bool,
    ) -> Result<(), Error>;
    /// Withdraw a deleted notification from the actual backend.
    /// # Errors
    /// Returns actual withdrawal failure.
    fn withdraw(&mut self, scope: Scope, notification: u32) -> Result<(), Error>;
    /// Send one admitted call action request to its source phone.
    /// # Errors
    /// Returns actual queue/backend failure, never a fabricated call result.
    fn request_action(&mut self, scope: Scope, notification: u32, action: u8) -> Result<(), Error>;
}
/// Source-phone call action lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationActionState {
    /// Reserved, no event/backend action yet.
    Prepared,
    /// Actual request submitted; phone result unknown.
    AwaitingResult,
    /// Result 1 executed, 2 denied, 3 missing, 4 timeout, 5 unsupported.
    Settled(u8),
    /// Submission failed, outcome not claimed as success.
    Failed,
    /// Source scope ended before a confirmed result; never replay automatically.
    Unknown,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Presentation {
    Prepared,
    Presented,
    Failed,
}
#[derive(Debug, Clone, Copy)]
struct Content<'a> {
    object_id: u32,
    value: Notification<'a>,
    presentation: Presentation,
    alert: bool,
}
#[derive(Debug, Clone, Copy)]
struct Pending {
    notification: u32,
    action: u8,
    state: NotificationActionState,
}
/// Fixed content capacity C and action ledger capacity A, scoped to one authenticated source phone.
pub struct NotificationService<'a, const C: usize, const A: usize> {
    scope: Scope,
    contents: [Option<Content<'a>>; C],
    actions: [Option<Pending>; A],
}
impl<const C: usize, const A: usize> core::fmt::Debug for NotificationService<'_, C, A> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("NotificationService")
            .field("content_length", &self.contents.iter().flatten().count())
            .field("action_length", &self.actions.iter().flatten().count())
            .finish_non_exhaustive()
    }
}
impl<'a, const C: usize, const A: usize> NotificationService<'a, C, A> {
    /// Empty service for a trusted owner and connection.
    #[must_use]
    pub const fn new(scope: Scope) -> Self {
        Self {
            scope,
            contents: [None; C],
            actions: [None; A],
        }
    }
    /// Register committed typed content without publishing a UI action before response admission.
    /// # Errors
    /// Rejects untrusted phone, invalid IDs/content, unsupported action bits or exhausted capacity.
    pub fn register(
        &mut self,
        object_id: u32,
        value: Notification<'a>,
        access: &impl AccessPort,
        port: &impl NotificationPort,
    ) -> Result<NotificationChange, Error> {
        access.authorize(self.scope, Operation::Notification)?;
        ensure(object_id != 0)?;
        value.validate().map_err(|_| Error::Invalid)?;
        let supported = port.supported_actions(self.scope)?;
        ensure(supported & !7 == 0 && value.action_mask & !supported == 0)?;
        if self
            .contents
            .iter()
            .flatten()
            .any(|c| (c.object_id == object_id) != (c.value.notification == value.notification))
        {
            return Err(Error::Conflict);
        }
        if let Some(c) = self
            .contents
            .iter_mut()
            .flatten()
            .find(|c| c.object_id == object_id)
        {
            if c.value == value {
                return Ok(NotificationChange::Unchanged);
            }
            c.value = value;
            c.presentation = Presentation::Prepared;
            c.alert = false;
            return Ok(NotificationChange::Updated);
        }
        let slot = self
            .contents
            .iter_mut()
            .find(|c| c.is_none())
            .ok_or(Error::Capacity)?;
        *slot = Some(Content {
            object_id,
            value,
            presentation: Presentation::Prepared,
            alert: true,
        });
        Ok(NotificationChange::New)
    }
    /// Publish committed content only after its successful response was admitted; identical retry is silent.
    /// # Errors
    /// Returns denied access, unknown object, absent backend or actual UI failure.
    pub fn content_response_admitted(
        &mut self,
        object_id: u32,
        access: &impl AccessPort,
        port: &mut impl NotificationPort,
    ) -> Result<(), Error> {
        access.authorize(self.scope, Operation::Notification)?;
        port.supported_actions(self.scope)?;
        let c = self
            .contents
            .iter_mut()
            .flatten()
            .find(|c| c.object_id == object_id)
            .ok_or(Error::NotFound)?;
        match c.presentation {
            Presentation::Presented => return Ok(()),
            Presentation::Failed => return Err(Error::Backend),
            Presentation::Prepared => {}
        }
        c.presentation = Presentation::Failed;
        port.present(self.scope, object_id, c.value, c.alert)?;
        c.presentation = Presentation::Presented;
        Ok(())
    }
    /// Reserve an incoming-call action only for its trusted registered source and supported action bit.
    /// # Errors
    /// Rejects unknown notification, non-call category, unsupported action, full ledger or untrusted phone.
    pub fn prepare_action(
        &mut self,
        notification: u32,
        action: u8,
        access: &impl AccessPort,
        port: &impl NotificationPort,
    ) -> Result<(), Error> {
        access.authorize(self.scope, Operation::Notification)?;
        ensure((1..=3).contains(&action))?;
        let supported = port.supported_actions(self.scope)?;
        let c = self
            .contents
            .iter()
            .flatten()
            .find(|c| c.value.notification == notification)
            .ok_or(Error::NotFound)?;
        if c.presentation != Presentation::Presented {
            return Err(Error::State);
        }
        let bit = 1u16 << (action - 1);
        if c.value.category != 1 || c.value.action_mask & bit == 0 || supported & bit == 0 {
            return Err(Error::Denied);
        }
        if self
            .actions
            .iter()
            .flatten()
            .any(|p| p.notification == notification && p.action == action)
        {
            return Ok(());
        }
        let slot = self
            .actions
            .iter_mut()
            .find(|p| p.is_none())
            .ok_or(Error::Capacity)?;
        *slot = Some(Pending {
            notification,
            action,
            state: NotificationActionState::Prepared,
        });
        Ok(())
    }
    /// Submit one admitted action. A repeated call never requests a second call operation.
    /// # Errors
    /// Returns unknown action, unavailable backend, denial, or actual submission failure.
    pub fn action_response_admitted(
        &mut self,
        notification: u32,
        action: u8,
        access: &impl AccessPort,
        port: &mut impl NotificationPort,
    ) -> Result<NotificationActionState, Error> {
        access.authorize(self.scope, Operation::Notification)?;
        ensure((1..=3).contains(&action))?;
        let supported = port.supported_actions(self.scope)?;
        if supported & (1u16 << (action - 1)) == 0 {
            return Err(Error::Unavailable);
        }
        let content = self
            .contents
            .iter()
            .flatten()
            .find(|content| content.value.notification == notification)
            .ok_or(Error::NotFound)?;
        if content.presentation != Presentation::Presented
            || content.value.category != 1
            || content.value.action_mask & (1u16 << (action - 1)) == 0
        {
            return Err(Error::Denied);
        }
        let scope = self.scope;
        let p = self.pending_mut(notification, action)?;
        if p.state != NotificationActionState::Prepared {
            return Ok(p.state);
        }
        p.state = NotificationActionState::Failed;
        port.request_action(scope, notification, action)?;
        p.state = NotificationActionState::AwaitingResult;
        Ok(p.state)
    }
    /// Accept the authenticated source phone's actual result 1..5; never answer a call here.
    /// # Errors
    /// Unknown tuple is `NotFound`, mismatched terminal result `Conflict`, unadmitted tuple `State`.
    pub fn settle(
        &mut self,
        notification: u32,
        action: u8,
        result: u8,
        access: &impl AccessPort,
    ) -> Result<(), Error> {
        access.authorize(self.scope, Operation::Settle)?;
        ensure((1..=3).contains(&action) && (1..=5).contains(&result))?;
        let p = self.pending_mut(notification, action)?;
        match p.state {
            NotificationActionState::AwaitingResult => {
                p.state = NotificationActionState::Settled(result);
                Ok(())
            }
            NotificationActionState::Settled(old) if old == result => Ok(()),
            NotificationActionState::Settled(_)
            | NotificationActionState::Failed
            | NotificationActionState::Unknown => Err(Error::Conflict),
            NotificationActionState::Prepared => Err(Error::State),
        }
    }
    /// Encode E82 for a known action tuple (5B).
    /// # Errors
    /// Returns unknown tuple or insufficient capacity.
    pub fn action_event(
        &self,
        notification: u32,
        action: u8,
        out: &mut [u8],
    ) -> Result<usize, Error> {
        if self.action_state(notification, action)? != NotificationActionState::Prepared {
            return Err(Error::State);
        }
        let mut w = Writer::new(out);
        w.u32(notification)?;
        w.u8(action)?;
        Ok(w.len())
    }
    /// Read an action's actual state.
    /// # Errors
    /// Returns `NotFound` for unknown notification/action.
    pub fn action_state(
        &self,
        notification: u32,
        action: u8,
    ) -> Result<NotificationActionState, Error> {
        self.actions
            .iter()
            .flatten()
            .find(|p| p.notification == notification && p.action == action)
            .map(|p| p.state)
            .ok_or(Error::NotFound)
    }
    /// Withdraw a committed deletion after response admission, clearing its pending actions and result ledger.
    /// # Errors
    /// Returns denied access, unknown object or actual backend withdrawal error. Cleanup still occurs on failure.
    pub fn delete_response_admitted(
        &mut self,
        object_id: u32,
        access: &impl AccessPort,
        port: &mut impl NotificationPort,
    ) -> Result<(), Error> {
        access.authorize(self.scope, Operation::Notification)?;
        let slot = self
            .contents
            .iter_mut()
            .find(|c| c.is_some_and(|c| c.object_id == object_id))
            .ok_or(Error::NotFound)?;
        let c = slot.take().ok_or(Error::NotFound)?;
        for p in &mut self.actions {
            if p.is_some_and(|p| p.notification == c.value.notification) {
                *p = None;
            }
        }
        port.withdraw(self.scope, c.value.notification)
    }
    /// Revoke source scope/CCCD: clear all content, pending actions and result ledger.
    /// The returned count describes unconfirmed outcomes as Unknown; integration must never replay them.
    #[must_use]
    pub fn invalidate(&mut self) -> NotificationInvalidation<A> {
        let mut entries = [None; A];
        let mut count = 0;
        for action in self.actions.iter().flatten() {
            if matches!(
                action.state,
                NotificationActionState::Prepared | NotificationActionState::AwaitingResult
            ) {
                entries[count] = Some((action.notification, action.action));
                count += 1;
            }
        }
        self.contents = [None; C];
        self.actions = [None; A];
        NotificationInvalidation { entries, count }
    }
    /// Replace owner/connection after clearing old state, reporting unconfirmed outcomes as Unknown.
    /// # Errors
    /// Rejects accidental same-scope reset.
    pub fn replace_scope(&mut self, scope: Scope) -> Result<NotificationInvalidation<A>, Error> {
        if scope == self.scope {
            return Err(Error::Conflict);
        }
        let pending = self.invalidate();
        self.scope = scope;
        Ok(pending)
    }
    fn pending_mut(&mut self, notification: u32, action: u8) -> Result<&mut Pending, Error> {
        self.actions
            .iter_mut()
            .flatten()
            .find(|p| p.notification == notification && p.action == action)
            .ok_or(Error::NotFound)
    }
}

/// Terminal Unknown outcomes retained for the caller while the service clears its old ledger.
#[derive(Clone, Copy)]
pub struct NotificationInvalidation<const A: usize> {
    entries: [Option<(u32, u8)>; A],
    count: usize,
}
impl<const A: usize> NotificationInvalidation<A> {
    /// Number of unconfirmed action tuples.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.count
    }
    /// Whether no unconfirmed actions existed.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.count == 0
    }
    /// Read an unconfirmed notification/action tuple and explicit Unknown result.
    /// # Errors
    /// Returns `NotFound` for an invalid index.
    pub fn outcome(&self, index: usize) -> Result<(u32, u8, NotificationActionState), Error> {
        let (notification, action) = self
            .entries
            .get(index)
            .copied()
            .flatten()
            .ok_or(Error::NotFound)?;
        Ok((notification, action, NotificationActionState::Unknown))
    }
}
impl<const A: usize> core::fmt::Debug for NotificationInvalidation<A> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("NotificationInvalidation")
            .field("length", &self.count)
            .finish_non_exhaustive()
    }
}
