//! Test-only actual-state fixtures. No fixture is linked into the production library.
use core::cell::Cell;
use device_content::{AccessPort, Scope};
use device_content_service::{Backend, ContentAccess, ContentClock, Error};
use device_protocol::{Frame, Header, Kind, ModuleEntry};
#[derive(Debug)]
pub(crate) struct Access {
    pub owner: Cell<u64>,
    pub connection: Cell<u64>,
    pub generation: Cell<u64>,
    pub enabled: Cell<bool>,
    pub coarse: Cell<bool>,
    #[cfg(feature = "notifications")]
    pub notification_generation: Cell<u64>,
    #[cfg(feature = "companion-radio")]
    pub revision: Cell<u32>,
    #[cfg(feature = "companion-radio")]
    pub property_generation: Cell<u64>,
}
impl Access {
    pub fn new() -> Self {
        Self {
            owner: Cell::new(1),
            connection: Cell::new(1),
            generation: Cell::new(1),
            enabled: Cell::new(true),
            coarse: Cell::new(true),
            #[cfg(feature = "notifications")]
            notification_generation: Cell::new(1),
            #[cfg(feature = "companion-radio")]
            revision: Cell::new(0),
            #[cfg(feature = "companion-radio")]
            property_generation: Cell::new(0),
        }
    }
}
impl AccessPort for Access {
    fn authorize(
        &self,
        scope: Scope,
        _: device_content::Operation,
    ) -> Result<(), device_content::Error> {
        if self.enabled.get()
            && self.coarse.get()
            && scope.owner() == self.owner.get()
            && scope.connection() == self.connection.get()
        {
            Ok(())
        } else {
            Err(device_content::Error::Denied)
        }
    }
}
impl ContentAccess for Access {
    fn scope(&self) -> Result<Scope, Error> {
        Scope::new(self.owner.get(), self.connection.get()).map_err(Error::from)
    }
    fn generation(&self) -> Result<u64, Error> {
        Ok(self.generation.get())
    }
    fn allowed(&self, _: u8, _: u8, _: Option<u32>) -> bool {
        self.enabled.get()
    }
    #[cfg(feature = "notifications")]
    fn notification_commit_allowed(&self, _: u32, _: u32) -> bool {
        self.enabled.get()
    }
    #[cfg(feature = "notifications")]
    fn notification_current(&self, _: u32, generation: u64) -> bool {
        generation > 0 && generation == self.notification_generation.get()
    }
    #[cfg(feature = "companion-radio")]
    fn radio_current(&self, revision: u32, generation: u64) -> bool {
        revision == self.revision.get() && generation == self.property_generation.get()
    }
}
#[derive(Debug)]
pub(crate) struct Clock(pub Cell<u64>);
impl ContentClock for Clock {
    fn now_ms(&self) -> Result<u64, Error> {
        Ok(self.0.get())
    }
}
#[cfg(any(feature = "phone", feature = "media"))]
#[derive(Debug)]
pub(crate) struct Actions<'a> {
    pub calls: u32,
    pub missing: bool,
    pub revoke: bool,
    access: &'a Access,
}
#[cfg(any(feature = "phone", feature = "media"))]
impl device_content::ActionPort for Actions<'_> {
    fn available(&self, _: device_content::Action) -> bool {
        !self.missing
    }
    fn execute(
        &mut self,
        _: Scope,
        _: u32,
        action: device_content::Action,
    ) -> Result<device_content::Submission, device_content::Error> {
        self.calls += 1;
        if self.revoke {
            self.access.enabled.set(false);
        }
        match action {
            #[cfg(feature = "media")]
            device_content::Action::CameraMode(_) | device_content::Action::DeviceMusic(_) => {
                Ok(device_content::Submission::Completed)
            }
            _ => Ok(device_content::Submission::AwaitingResult),
        }
    }
}
#[cfg(feature = "find")]
#[derive(Debug, Default)]
pub(crate) struct Find {
    pub starts: u32,
    pub stops: u32,
    pub deadline: u64,
    pub missing: bool,
    pub stop_fail: bool,
    pub start_error: Option<device_content::Error>,
}
#[cfg(feature = "find")]
impl device_content::FindPort for Find {
    fn capability(
        &self,
        _: device_content::FindTarget,
    ) -> Result<device_content::FindCapability, device_content::Error> {
        if self.missing {
            return Err(device_content::Error::Unavailable);
        }
        Ok(device_content::FindCapability {
            patterns: 3,
            min_duration_s: 1,
            max_duration_s: 60,
        })
    }
    fn start(
        &mut self,
        _: Scope,
        _: u32,
        _: device_content::FindRequest,
        deadline: u64,
    ) -> Result<(), device_content::Error> {
        self.starts += 1;
        self.deadline = deadline;
        if let Some(error) = self.start_error {
            return Err(error);
        }
        Ok(())
    }
    fn stop(&mut self, _: Scope, _: u32) -> Result<(), device_content::Error> {
        self.stops += 1;
        if self.stop_fail {
            return Err(device_content::Error::Backend);
        }
        Ok(())
    }
}
#[cfg(feature = "notifications")]
#[derive(Debug, Default)]
pub(crate) struct Notifications {
    pub alerts: u32,
    pub presents: u32,
    pub requests: u32,
    pub withdrawals: u32,
}
#[cfg(feature = "notifications")]
impl device_content::NotificationPort for Notifications {
    fn supported_actions(&self, _: Scope) -> Result<u16, device_content::Error> {
        Ok(7)
    }
    fn present(
        &mut self,
        _: Scope,
        _: u32,
        _: device_content::Notification<'_>,
        alert: bool,
    ) -> Result<(), device_content::Error> {
        self.presents += 1;
        if alert {
            self.alerts += 1;
        }
        Ok(())
    }
    fn withdraw(&mut self, _: Scope, _: u32) -> Result<(), device_content::Error> {
        self.withdrawals += 1;
        Ok(())
    }
    fn request_action(&mut self, _: Scope, _: u32, _: u8) -> Result<(), device_content::Error> {
        self.requests += 1;
        Ok(())
    }
}
#[cfg(feature = "companion-radio")]
#[derive(Debug, Default)]
pub(crate) struct Radio {
    pub starts: u32,
    pub cancels: u32,
    pub stable: bool,
    pub desired: bool,
}
#[cfg(feature = "companion-radio")]
impl device_content::CompanionRadioPort for Radio {
    fn available(&self) -> bool {
        true
    }
    fn set_enabled(
        &mut self,
        _: Scope,
        desired: bool,
    ) -> Result<device_content::BackendTicket, device_content::Error> {
        self.starts += 1;
        self.desired = desired;
        device_content::BackendTicket::new(self.starts)
    }
    fn poll(
        &mut self,
        _: Scope,
        _: device_content::BackendTicket,
    ) -> Result<device_content::ClassicBluetooth, device_content::Error> {
        Ok(device_content::ClassicBluetooth {
            enabled: self.desired,
            state: if self.stable {
                if self.desired { 3 } else { 1 }
            } else if self.desired {
                2
            } else {
                5
            },
            address: [0; 6],
            valid: false,
        })
    }
    fn cancel(
        &mut self,
        _: Scope,
        _: device_content::BackendTicket,
    ) -> Result<(), device_content::Error> {
        self.cancels += 1;
        Ok(())
    }
}
#[cfg(feature = "companion-radio")]
impl device_content_service::ClassicPort for Radio {
    fn observe(
        &mut self,
        _: Scope,
    ) -> Result<device_content::ClassicBluetooth, device_content::Error> {
        Ok(device_content::ClassicBluetooth {
            enabled: self.desired,
            state: if self.stable {
                if self.desired { 3 } else { 1 }
            } else if self.desired {
                2
            } else {
                5
            },
            address: [0; 6],
            valid: false,
        })
    }
}
#[cfg(feature = "companion-radio")]
#[derive(Debug)]
pub(crate) struct RadioStore<'a> {
    pub publishes: u32,
    last: Option<device_content::ClassicBluetooth>,
    access: &'a Access,
}
#[cfg(feature = "companion-radio")]
impl device_content_service::RadioStateStore for RadioStore<'_> {
    fn publish(
        &mut self,
        _: Scope,
        value: device_content::ClassicBluetooth,
    ) -> Result<device_content_service::PropertySnapshot, Error> {
        if self.last != Some(value) {
            self.publishes += 1;
            self.last = Some(value);
            self.access.revision.set(self.publishes);
            self.access
                .property_generation
                .set(u64::from(self.publishes));
        }
        Ok(device_content_service::PropertySnapshot {
            revision: self.access.revision.get(),
            generation: self.access.property_generation.get(),
        })
    }
}
#[cfg(feature = "gimbal")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Observation {
    Running,
    Terminal,
    Failed,
}
#[cfg(feature = "gimbal")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Cancellation {
    Accepted,
    Failed,
}
#[cfg(feature = "gimbal")]
#[derive(Debug)]
pub(crate) struct Gimbal {
    pub starts: u32,
    pub polls: u32,
    pub cancels: u32,
    pub observation: Observation,
    pub ready: bool,
    pub drained: bool,
    pub drain_after_cancel: bool,
    pub scope_cancels: u32,

    pub cancellation: Cancellation,
    pub drain_error: Option<device_content::Error>,
}
#[cfg(feature = "gimbal")]
impl Default for Gimbal {
    fn default() -> Self {
        Self {
            starts: 0,
            polls: 0,
            cancels: 0,
            observation: Observation::Running,
            ready: true,
            drained: true,
            drain_after_cancel: true,
            scope_cancels: 0,
            cancellation: Cancellation::Accepted,
            drain_error: None,
        }
    }
}
#[cfg(feature = "gimbal")]
impl device_content_service::GimbalLifecyclePort for Gimbal {
    fn ready(
        &self,
        _: Scope,
        _: device_content::GimbalRequest,
    ) -> Result<bool, device_content::Error> {
        Ok(self.ready)
    }
    fn cancel_scope(&mut self, _: Scope) -> Result<(), device_content::Error> {
        self.scope_cancels += 1;
        if self.cancellation == Cancellation::Failed {
            return Err(device_content::Error::Backend);
        }
        if self.drain_after_cancel {
            self.drained = true;
        }
        Ok(())
    }
    fn drained(&self, _: Scope) -> Result<bool, device_content::Error> {
        if let Some(error) = self.drain_error {
            return Err(error);
        }
        Ok(self.drained)
    }
}
#[cfg(feature = "gimbal")]
impl device_content::GimbalPort for Gimbal {
    fn capability(&self) -> Result<device_content::GimbalCapability, device_content::Error> {
        Ok(device_content::GimbalCapability {
            revision: 1,
            modes: 3,
            directions: 3,
            min_duration_ms: 1,
            max_duration_ms: 10_000,
            min_angle_centi_deg: 1,
            max_angle_centi_deg: 36_000,
        })
    }
    fn submit(
        &mut self,
        _: Scope,
        _: device_content::GimbalRequest,
    ) -> Result<device_content::BackendTicket, device_content::Error> {
        self.starts += 1;
        self.drained = false;
        device_content::BackendTicket::new(self.starts)
    }
    fn poll(
        &mut self,
        _: Scope,
        _: device_content::BackendTicket,
    ) -> Result<device_content::GimbalObservation, device_content::Error> {
        self.polls += 1;
        if self.observation == Observation::Failed {
            return Err(device_content::Error::Backend);
        }
        if self.observation == Observation::Terminal {
            self.drained = true;
            Ok(device_content::GimbalObservation::Terminal {
                result: device_content::GimbalResult::Completed,
                reason: 0,
            })
        } else {
            Ok(device_content::GimbalObservation::Running)
        }
    }
    fn cancel(
        &mut self,
        _: Scope,
        _: device_content::BackendTicket,
    ) -> Result<(), device_content::Error> {
        self.cancels += 1;
        if self.cancellation == Cancellation::Failed {
            return Err(device_content::Error::Backend);
        }
        Ok(())
    }
}
#[derive(Debug)]
pub(crate) struct Ports<'a> {
    #[cfg(any(feature = "phone", feature = "media"))]
    pub actions: Actions<'a>,
    #[cfg(feature = "find")]
    pub find: Find,
    #[cfg(feature = "notifications")]
    pub notifications: Notifications,
    #[cfg(feature = "companion-radio")]
    pub radio: Radio,
    #[cfg(feature = "companion-radio")]
    pub store: RadioStore<'a>,
    #[cfg(feature = "gimbal")]
    pub gimbal: Gimbal,
    marker: core::marker::PhantomData<&'a ()>,
}
impl<'a> Ports<'a> {
    pub fn new(access: &'a Access) -> Self {
        let _ = access;
        Self {
            #[cfg(any(feature = "phone", feature = "media"))]
            actions: Actions {
                calls: 0,
                missing: false,
                revoke: false,
                access,
            },
            #[cfg(feature = "find")]
            find: Find::default(),
            #[cfg(feature = "notifications")]
            notifications: Notifications::default(),
            #[cfg(feature = "companion-radio")]
            radio: Radio::default(),
            #[cfg(feature = "companion-radio")]
            store: RadioStore {
                publishes: 0,
                last: None,
                access,
            },
            #[cfg(feature = "gimbal")]
            gimbal: Gimbal::default(),
            marker: core::marker::PhantomData,
        }
    }
}
#[cfg(any(feature = "phone", feature = "media", feature = "companion-radio"))]
impl<'a> Backend for Ports<'a> {
    #[cfg(any(feature = "phone", feature = "media"))]
    type Actions = Actions<'a>;
    #[cfg(any(feature = "phone", feature = "media"))]
    fn actions(&mut self) -> &mut Self::Actions {
        &mut self.actions
    }
    #[cfg(feature = "find")]
    type Find = Find;
    #[cfg(feature = "find")]
    fn find(&mut self) -> &mut Find {
        &mut self.find
    }
    #[cfg(feature = "notifications")]
    type Notifications = Notifications;
    #[cfg(feature = "notifications")]
    fn notifications(&mut self) -> &mut Notifications {
        &mut self.notifications
    }
    #[cfg(feature = "companion-radio")]
    type Radio = Radio;
    #[cfg(feature = "companion-radio")]
    fn radio(&mut self) -> &mut Radio {
        &mut self.radio
    }
    #[cfg(feature = "companion-radio")]
    type RadioState = RadioStore<'a>;
    #[cfg(feature = "companion-radio")]
    fn radio_state(&mut self) -> &mut Self::RadioState {
        &mut self.store
    }
    #[cfg(feature = "gimbal")]
    type Gimbal = Gimbal;
    #[cfg(feature = "gimbal")]
    fn gimbal(&mut self) -> &mut Gimbal {
        &mut self.gimbal
    }
}
#[cfg(not(any(feature = "phone", feature = "media", feature = "companion-radio")))]
impl Backend for Ports<'_> {
    #[cfg(any(feature = "phone", feature = "media"))]
    type Actions = Actions<'a>;
    #[cfg(any(feature = "phone", feature = "media"))]
    fn actions(&mut self) -> &mut Self::Actions {
        &mut self.actions
    }
    #[cfg(feature = "find")]
    type Find = Find;
    #[cfg(feature = "find")]
    fn find(&mut self) -> &mut Find {
        &mut self.find
    }
    #[cfg(feature = "notifications")]
    type Notifications = Notifications;
    #[cfg(feature = "notifications")]
    fn notifications(&mut self) -> &mut Notifications {
        &mut self.notifications
    }
    #[cfg(feature = "companion-radio")]
    type Radio = Radio;
    #[cfg(feature = "companion-radio")]
    fn radio(&mut self) -> &mut Radio {
        &mut self.radio
    }
    #[cfg(feature = "companion-radio")]
    type RadioState = RadioStore<'a>;
    #[cfg(feature = "companion-radio")]
    fn radio_state(&mut self) -> &mut Self::RadioState {
        &mut self.store
    }
    #[cfg(feature = "gimbal")]
    type Gimbal = Gimbal;
    #[cfg(feature = "gimbal")]
    fn gimbal(&mut self) -> &mut Gimbal {
        &mut self.gimbal
    }
}
pub(crate) fn entries() -> Vec<ModuleEntry> {
    [
        (cfg!(feature = "find"), 6),
        (cfg!(feature = "phone"), 7),
        (cfg!(feature = "companion-radio"), 8),
        (cfg!(feature = "notifications"), 0x30),
        (cfg!(any(feature = "media", feature = "gimbal")), 0x33),
    ]
    .into_iter()
    .filter(|value| value.0)
    .map(|(_, module)| ModuleEntry {
        module,
        major: 1,
        minor: 0,
        availability: 1,
        features: 0,
        input_max: 128,
        output_max: 128,
    })
    .collect()
}
pub(crate) fn frame(
    module: u8,
    opcode: u8,
    message_id: u16,
    payload: &[u8],
) -> Result<Frame<'_>, Error> {
    Ok(Frame {
        header: Header {
            kind: Kind::Request,
            module,
            opcode,
            message_id,
            offset: 0,
            total_length: u16::try_from(payload.len()).map_err(|_| Error::Capacity)?,
        },
        payload,
    })
}
pub(crate) fn binding<'a>(
    access: &Access,
    entries: &'a [ModuleEntry],
) -> Result<device_content_service::Binding<'a>, Error> {
    Ok(device_content_service::Binding {
        scope: access.scope()?,
        entries,
        #[cfg(any(feature = "phone", feature = "notifications"))]
        manifest: device_content::Manifest {
            string_max: Some(64),
            object_max: Some(512),
            count_max: Some(8),
            heartbeat_timeout_s: Some(device_content::Range { min: 1, max: 120 }),
            heartbeat_grace_s: Some(device_content::Range { min: 0, max: 20 }),
            ..device_content::Manifest::default()
        },
    })
}
