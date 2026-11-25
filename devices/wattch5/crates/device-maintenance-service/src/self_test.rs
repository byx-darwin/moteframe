use crate::common::{Guard, entry, failure, frame, id32, put16, put32, status, u16_at, u32_at};
use crate::{CleanupTrace, Clock, Error, SelfTestPortObserver};
use core::cell::{Cell, RefCell};
use device_maintenance::{
    Authority, Capability, Profile,
    self_test::{EvidenceVerifier, ExternalRequest, JobState, SelfTestEngine, TestSpec, Verdict},
};
use device_protocol::{Frame, ModuleEntry};
use device_services::{Reason, ServiceHandler, SessionError, Status};
/// Product-declared static LIST flags; method is taken from the exact engine `TestSpec` manifest.
pub trait TestListFlags {
    /// # Errors
    /// Missing actual declaration is unavailable; unknown flag bits are rejected by the handler.
    fn flags(&self, test: u16) -> Result<u8, Error>;
}
#[derive(Clone, Copy)]
struct Catalog {
    test: u16,
    method: u8,
    flags: u8,
}
impl Catalog {
    const EMPTY: Self = Self {
        test: 0,
        method: 0,
        flags: 0,
    };
}
/// Controlled71 wire adapter. Constructed with the same actual `TestSpec` used by its engine.
pub struct SelfTestHandler<'a> {
    metadata: ModuleEntry,
    engine: RefCell<SelfTestEngine<'a>>,
    catalog: [Catalog; 254],
    count: usize,
    specs: &'a [TestSpec<'a>],
    authority: &'a dyn Authority,
    station: u32,
    verifier: &'a dyn EvidenceVerifier,
    guard: Guard<'a>,
    pending: Cell<Option<(u16, u32)>>,
    active: Cell<Option<(u32, bool)>>,
    cleaned: Cell<bool>,
    cleanup: Cell<Option<Error>>,
    trace: &'a CleanupTrace,
}
impl core::fmt::Debug for SelfTestHandler<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SelfTestHandler")
            .field("catalog_count", &self.count)
            .finish_non_exhaustive()
    }
}
impl<'a> SelfTestHandler<'a> {
    /// # Errors
    /// Rejects commercial profile, failed real cleanup evidence, missing authenticated
    /// station/criteria/backend and invalid metadata. The observed port must use its own trace.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        metadata: ModuleEntry,
        profile: Profile,
        port: &'a mut SelfTestPortObserver<'_>,
        specs: &'a [TestSpec<'a>],
        authority: &'a dyn Authority,
        clock: &'a dyn Clock,
        flags: &dyn TestListFlags,
        verifier: &'a dyn EvidenceVerifier,
    ) -> Result<Self, Error> {
        entry(metadata, 0x71, 1, 1)?;
        let station = authority
            .station_id()
            .filter(|id| *id != 0)
            .ok_or(Error::Denied)?;
        let trace = port.trace;
        if let Some(error) = trace.failure() {
            return Err(error);
        }
        let mut engine = SelfTestEngine::new(profile, Some(port), specs, authority)?;
        let mut catalog = [Catalog::EMPTY; 254];
        let mut count = 0;
        let mut start = 0;
        loop {
            let mut ids = [0; 8];
            let (next, n) = engine.list(start, 8, &mut ids)?;
            for id in &ids[..n] {
                let spec = specs.iter().find(|s| s.id == *id).ok_or(Error::Invalid)?;
                let value = flags.flags(*id)?;
                if value & !1 != 0 || count >= 254 {
                    return Err(Error::Invalid);
                }
                catalog[count] = Catalog {
                    test: *id,
                    method: spec.method as u8,
                    flags: value,
                };
                count += 1;
            }
            if next == 255 {
                break;
            }
            start = next;
        }
        Ok(Self {
            metadata,
            engine: RefCell::new(engine),
            catalog,
            count,
            specs,
            authority,
            station,
            verifier,
            guard: Guard::new(clock),
            pending: Cell::new(None),
            active: Cell::new(None),
            cleaned: Cell::new(false),
            cleanup: Cell::new(None),
            trace,
        })
    }
    fn scope(&self) -> bool {
        self.authority.station_id() == Some(self.station)
            && self.authority.allows(Capability::SelfTest)
            && self.active.get().is_none_or(|(_, danger)| {
                !danger || self.authority.allows(Capability::DangerousSelfTest)
            })
    }
    fn close(&self) -> Result<(), Error> {
        self.guard.close();
        self.pending.set(None);
        self.active.set(None);
        if self.cleaned.get() {
            return self.cleanup.get().map_or(Ok(()), Err);
        }
        let mut engine = self.engine.try_borrow_mut().map_err(|_| Error::Busy)?;
        let result = engine.disconnect().map_err(Error::from);
        let error = self.trace.failure().or(result.err());
        self.cleaned.set(true);
        self.cleanup.set(error);
        error.map_or(Ok(()), Err)
    }
    fn check(&self) -> Result<u64, Error> {
        if let Some(error) = self.trace.failure() {
            let _ = self.close();
            return Err(error);
        }
        if !self.scope() {
            let _ = self.close();
            return Err(Error::Denied);
        }
        let now = self.guard.now();
        if now.is_err() {
            let _ = self.close();
        }
        now
    }
    /// Drive actual controlled observations/timeouts on the application executor.
    /// # Errors
    /// Backend, scope, criterion, cleanup and trusted-clock failures remain explicit.
    pub fn poll(&mut self) -> Result<(), Error> {
        let now = self.check()?;
        let active = self.active.get();
        let ended = self.with_engine(|engine| {
            engine.poll(now)?;
            Ok(active.is_some_and(|(job, _)| {
                engine
                    .result(job, now)
                    .is_ok_and(|result| terminal(result.state))
            }))
        })?;
        if ended {
            self.active.set(None);
        }
        Ok(())
    }
    fn with_engine<T>(
        &self,
        action: impl FnOnce(&mut SelfTestEngine<'a>) -> Result<T, device_maintenance::Error>,
    ) -> Result<T, Error> {
        let result = {
            let mut engine = self.engine.try_borrow_mut().map_err(|_| Error::Busy)?;
            action(&mut engine).map_err(Error::from)
        };
        if let Some(error) = self.trace.failure() {
            let _ = self.close();
            return Err(error);
        }
        if matches!(
            result,
            Err(Error::Engine(device_maintenance::Error::Closed))
        ) {
            let _ = self.close();
        }
        result
    }
    fn settled(&self, job: u32) {
        if self.active.get().is_some_and(|(id, _)| id == job) {
            self.active.set(None);
        }
    }
    fn list(&self, payload: &[u8], out: &mut [u8]) -> Result<usize, Error> {
        if payload.len() != 2 || payload[0] == 255 || !(1..=8).contains(&payload[1]) {
            return Err(Error::Invalid);
        }
        let start = usize::from(payload[0]);
        let count = self
            .count
            .saturating_sub(start)
            .min(usize::from(payload[1]));
        let n = 6 + count * 4;
        if out.len() < n {
            return Err(Error::Capacity);
        }
        out[4] = if start + count >= self.count {
            255
        } else {
            u8::try_from(start + count).map_err(|_| Error::Invalid)?
        };
        out[5] = u8::try_from(count).map_err(|_| Error::Invalid)?;
        for (i, c) in self.catalog[start.min(self.count)..start.min(self.count) + count]
            .iter()
            .enumerate()
        {
            let p = 6 + i * 4;
            put16(out, p, c.test);
            out[p + 2] = c.method;
            out[p + 3] = c.flags;
        }
        status(out, Status::Ok, Reason::None);
        Ok(n)
    }
    fn result(&self, payload: &[u8], out: &mut [u8], now: u64) -> Result<usize, Error> {
        let job = id32(payload)?;
        if self.pending.get().is_some_and(|(_, j)| j == job) {
            return Err(Error::Unavailable);
        }
        if out.len() < 97 {
            return Err(Error::Capacity);
        }
        let r = self.with_engine(|engine| engine.result(job, now))?;
        if terminal(r.state) {
            self.settled(job);
        }
        let n = 33 + 8 * r.values().len();
        out[4] = r.state as u8;
        put16(out, 5, r.reason);
        put16(out, 7, r.test_id);
        put16(out, 9, r.criterion_rev);
        out[11] = r.origin as u8;
        put32(out, 12, r.station_id);
        out[16..32].copy_from_slice(&r.evidence);
        out[32] = u8::try_from(r.values().len()).map_err(|_| Error::Invalid)?;
        for (i, v) in r.values().iter().enumerate() {
            let p = 33 + i * 8;
            put16(out, p, v.id);
            out[p + 2..p + 6].copy_from_slice(&v.value.to_le_bytes());
            out[p + 6] = v.scale_exp.to_le_bytes()[0];
            out[p + 7] = v.unit;
        }
        status(out, Status::Ok, Reason::None);
        Ok(n)
    }
    fn execute(&mut self, f: Frame<'_>, out: &mut [u8], now: u64) -> Result<usize, Error> {
        match f.header.opcode {
            1 => self.list(f.payload, out),
            2 => {
                if f.payload.len() != 8 {
                    return Err(Error::Invalid);
                }
                if out.len() < 8 {
                    return Err(Error::Capacity);
                }
                if self.pending.get().is_some() {
                    return Err(Error::Busy);
                }
                let test = u16_at(f.payload, 0);
                let spec = self
                    .specs
                    .iter()
                    .find(|s| s.id == test)
                    .ok_or(Error::Invalid)?;
                if spec.dangerous && !self.authority.allows(Capability::DangerousSelfTest) {
                    return Err(Error::DangerousDenied);
                }
                let job = self.with_engine(|engine| {
                    engine.run(test, u16_at(f.payload, 2), u32_at(f.payload, 4), now)
                })?;
                self.pending.set(Some((f.header.message_id, job)));
                self.active.set(Some((job, spec.dangerous)));
                status(out, Status::Ok, Reason::None);
                put32(out, 4, job);
                Ok(8)
            }
            3 => self.result(f.payload, out, now),
            4 => {
                let job = id32(f.payload)?;
                self.with_engine(|engine| engine.cancel(job, now))?;
                self.settled(job);
                status(out, Status::Ok, Reason::None);
                Ok(4)
            }
            5 => {
                if f.payload.len() != 27 {
                    return Err(Error::Invalid);
                }
                let mut evidence = [0; 16];
                evidence.copy_from_slice(&f.payload[11..27]);
                let result = match f.payload[6] {
                    1 => Verdict::Pass,
                    2 => Verdict::Fail,
                    _ => return Err(Error::Invalid),
                };
                let r = ExternalRequest {
                    job: u32_at(f.payload, 0),
                    test: u16_at(f.payload, 4),
                    result,
                    station_id: u32_at(f.payload, 7),
                    evidence,
                };
                if r.job == 0 || r.test == 0 {
                    return Err(Error::Invalid);
                }
                self.with_engine(|engine| engine.external_result(r, self.verifier, now))?;
                self.settled(r.job);
                status(out, Status::Ok, Reason::None);
                Ok(4)
            }
            _ => Err(Error::Unsupported),
        }
    }
}
impl ServiceHandler for SelfTestHandler<'_> {
    fn required_features(&self, e: &ModuleEntry, op: u8) -> Option<u32> {
        (*e == self.metadata && matches!(op, 1..=5)).then_some(0)
    }
    fn authorized(&self, _: Frame<'_>) -> bool {
        !self.guard.closed() && self.scope()
    }
    fn must_end_session(&self) -> bool {
        if !self.scope() || self.trace.failure().is_some() {
            let _ = self.close();
        }
        self.guard.closed()
    }
    fn replay_valid(&self, f: Frame<'_>, response: &[u8]) -> bool {
        if self.guard.closed() || !self.scope() || response.len() < 4 {
            return false;
        }
        if response[..2] != [0, 0] {
            return true;
        }
        let job = match f.header.opcode {
            2 if response.len() == 8 => Ok(u32_at(response, 4)),
            3 | 4 => id32(f.payload),
            5 if f.payload.len() == 27 => Ok(u32_at(f.payload, 0)),
            1 => return true,
            _ => return false,
        };
        let Ok(job) = job else { return false };
        let Ok(now) = self.check() else { return false };
        self.with_engine(|engine| engine.result(job, now)).is_ok()
    }
    fn response_admitted(&mut self, f: Frame<'_>, response: &[u8]) -> Result<(), SessionError> {
        self.check().map_err(crate::common::local)?;
        if f.header.opcode == 2
            && let Some((request, job)) = self.pending.get()
        {
            if request != f.header.message_id
                || response.len() != 8
                || response[..4] != [0; 4]
                || u32_at(response, 4) != job
            {
                return Err(SessionError::InvalidHandlerResponse);
            }
            self.pending.set(None);
        }
        Ok(())
    }
    fn connection_closed(&mut self) -> Result<(), SessionError> {
        self.close().map_err(crate::common::local)
    }
    fn dispatch(&mut self, f: Frame<'_>, out: &mut [u8]) -> Result<usize, SessionError> {
        frame(f, self.metadata)?;
        if self.must_end_session() {
            out.fill(0);
            return Err(SessionError::AuthorizationRevoked);
        }
        let now = self.check().map_err(crate::common::local)?;
        if out.len() < 4 {
            return Err(SessionError::Capacity);
        }
        let result = self.execute(f, out, now);
        if !self.authorized(f) {
            out.fill(0);
            return Err(SessionError::AuthorizationRevoked);
        }
        match result {
            Ok(n) => Ok(n),
            Err(Error::Capacity) => Err(SessionError::Capacity),
            Err(e) => Ok(failure(out, e)),
        }
    }
}
fn terminal(s: JobState) -> bool {
    matches!(
        s,
        JobState::Passed
            | JobState::Failed
            | JobState::Unsupported
            | JobState::TimedOut
            | JobState::Cancelled
    )
}
