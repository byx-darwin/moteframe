use crate::common::{Guard, entry, failure, frame, id16, put16, put32, status};
use crate::{Clock, Error, NamedRequest};
use core::cell::{Cell, RefCell};
use device_maintenance::{
    Authority, Capability, Profile,
    persistent::{
        NamedOperation, OperationSchema, PersistentEngine, PersistentRecord, PersistentState,
        StorageBackend,
    },
};
use device_protocol::{Frame, ModuleEntry};
use device_services::{Reason, ServiceHandler, SessionError, Status};
/// Real executor on the same actual persistent ledger as `StorageBackend`. It must atomically claim
/// Accepted->Running at response admission and never replay Running/terminal physical operations.
pub trait PersistentMaintenanceExecutor {
    fn available(&self, module: u8, operation: u16) -> bool;
    /// Real scope includes the original immutable policy revision, per-operation permission and D.
    fn authorized(&self, record: PersistentRecord, write: bool, authority: &dyn Authority) -> bool;
    fn binding_current(&self, record: PersistentRecord) -> bool;
    /// # Errors
    /// Real/unknown execution failures are explicit. Recheck scope and full persisted binding inside
    /// the same actual ledger serialization boundary before any physical effect.
    fn response_admitted(
        &mut self,
        record: PersistentRecord,
        authority: &dyn Authority,
    ) -> Result<(), device_maintenance::Error>;
}
///72/73 wire adapter. Actual storage, recovery, product schemas and executor are all mandatory.
pub struct PersistentMaintenanceHandler<'a> {
    metadata: ModuleEntry,
    engine: RefCell<PersistentEngine<'a>>,
    schema: &'a dyn OperationSchema,
    executor: RefCell<&'a mut dyn PersistentMaintenanceExecutor>,
    authority: &'a dyn Authority,
    station: u32,
    guard: Guard<'a>,
    outputs: bool,
    seen: Cell<bool>,
    pending_danger: Cell<bool>,
    context_danger: Cell<bool>,
    pending: Cell<Option<(u16, PersistentRecord)>>,
    context: Cell<Option<(bool, PersistentRecord)>>,
    last_admitted: Cell<Option<u16>>,
}
impl core::fmt::Debug for PersistentMaintenanceHandler<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("PersistentMaintenanceHandler")
            .field("module", &self.metadata.module)
            .finish_non_exhaustive()
    }
}
impl<'a> PersistentMaintenanceHandler<'a> {
    /// outputs is an explicit trusted product-manifest choice selecting the implemented optional
    /// original-policy output schema; it never relaxes `PersistentEngine` output validation.
    /// # Errors
    /// Rejects commercial profile, disabled module, missing durable guarantees/source or bad metadata.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        metadata: ModuleEntry,
        profile: Profile,
        backend: &'a mut dyn StorageBackend,
        schema: &'a dyn OperationSchema,
        executor: &'a mut dyn PersistentMaintenanceExecutor,
        authority: &'a dyn Authority,
        clock: &'a dyn Clock,
        outputs: bool,
    ) -> Result<Self, Error> {
        let module = metadata.module;
        if !((module == 0x72 && cfg!(feature = "engineering"))
            || (module == 0x73 && cfg!(feature = "factory-config")))
        {
            return Err(Error::Unsupported);
        }
        entry(metadata, module, 1, 0)?;
        let station = authority
            .station_id()
            .filter(|v| *v != 0)
            .ok_or(Error::Denied)?;
        let engine = PersistentEngine::new(profile, Some(backend), schema, authority)?;
        let capability = if module == 0x72 {
            Capability::Engineering
        } else {
            Capability::FactoryConfig
        };
        Ok(Self {
            metadata,
            engine: RefCell::new(engine),
            schema,
            executor: RefCell::new(executor),
            authority,
            station,
            guard: Guard::new(clock),
            outputs,
            seen: Cell::new(authority.allows(capability)),
            pending_danger: Cell::new(false),
            context_danger: Cell::new(false),
            pending: Cell::new(None),
            context: Cell::new(None),
            last_admitted: Cell::new(None),
        })
    }
    fn scope(&self) -> bool {
        self.authority.station_id() == Some(self.station)
            && self.authority.allows(if self.metadata.module == 0x72 {
                Capability::Engineering
            } else {
                Capability::FactoryConfig
            })
    }
    fn close(&self) {
        self.guard.close();
        self.pending.set(None);
        self.context.set(None);
    }
    fn context_allowed(&self, write: bool, r: PersistentRecord) -> bool {
        self.executor
            .try_borrow()
            .is_ok_and(|x| x.authorized(r, write, self.authority) && x.binding_current(r))
    }
    fn query(&self, payload: &[u8], out: &mut [u8]) -> Result<usize, Error> {
        let id = id16(payload)?;
        if out.len() < 9 {
            return Err(Error::Capacity);
        }
        let r = self
            .engine
            .try_borrow_mut()
            .map_err(|_| Error::Busy)?
            .query(self.metadata.module, id)?;
        if !self.context_allowed(false, r) {
            return Err(Error::Denied);
        }
        self.context_danger.set(false);
        self.context.set(Some((false, r)));
        status(out, Status::Ok, Reason::None);
        out[4] = r.state as u8;
        put16(out, 5, r.result);
        put16(out, 7, r.reason);
        Ok(9)
    }
    fn output(&self, payload: &[u8], out: &mut [u8]) -> Result<usize, Error> {
        let id = id16(payload)?;
        if out.len() < 10 {
            return Err(Error::Capacity);
        }
        let r = self
            .engine
            .try_borrow_mut()
            .map_err(|_| Error::Busy)?
            .query(self.metadata.module, id)?;
        if !self.context_allowed(false, r) {
            return Err(Error::Denied);
        }
        let output = self
            .engine
            .try_borrow_mut()
            .map_err(|_| Error::Busy)?
            .output(self.metadata.module, id, &mut out[10..])?;
        if output.length > out.len() - 10 {
            return Err(Error::Storage);
        }
        if crate::NamedOutput::decode(
            self.metadata.module,
            output.operation,
            output.schema,
            &out[10..10 + output.length],
        )
        .is_err()
        {
            out.fill(0);
            return Err(Error::Storage);
        }
        self.context_danger.set(false);
        self.context.set(Some((false, r)));
        status(out, Status::Ok, Reason::None);
        put16(out, 4, output.operation);
        put16(out, 6, output.schema);
        put16(
            out,
            8,
            u16::try_from(output.length).map_err(|_| Error::Capacity)?,
        );
        Ok(10 + output.length)
    }
    fn execute(&mut self, f: Frame<'_>, out: &mut [u8]) -> Result<usize, Error> {
        match f.header.opcode {
            0x40 => {
                let r = NamedRequest::decode(f.payload)?;
                validate_params(self.metadata.module, r)?;
                if out.len() < 8 {
                    return Err(Error::Capacity);
                }
                let operation = NamedOperation {
                    module: self.metadata.module,
                    operation: r.operation,
                    operation_id: r.operation_id,
                    params: r.params,
                };
                let policy = self.schema.validate(operation)?;
                if policy.revision == 0 {
                    return Err(Error::Unavailable);
                }
                let dangerous = policy.dangerous
                    || self.metadata.module == 0x73
                    || (self.metadata.module == 0x72
                        && matches!(r.operation, 1 | 2 | 7 | 8 | 10 | 11));
                if dangerous && !self.authority.allows(Capability::DangerousMaintenance) {
                    return Err(Error::DangerousDenied);
                }
                if !self
                    .executor
                    .try_borrow()
                    .map_err(|_| Error::Busy)?
                    .available(self.metadata.module, r.operation)
                {
                    return Err(Error::Unsupported);
                }
                let claim = self
                    .engine
                    .try_borrow_mut()
                    .map_err(|_| Error::Busy)?
                    .submit(NamedOperation {
                        module: self.metadata.module,
                        operation: r.operation,
                        operation_id: r.operation_id,
                        params: r.params,
                    })?;
                if !self.context_allowed(true, claim.record) {
                    self.close();
                    return Err(Error::Denied);
                }
                self.pending_danger.set(dangerous);
                self.context_danger.set(dangerous);
                self.pending.set(Some((f.header.message_id, claim.record)));
                self.context.set(Some((true, claim.record)));
                status(out, Status::Ok, Reason::None);
                put32(out, 4, claim.record.job_id);
                Ok(8)
            }
            0x41 => self.query(f.payload, out),
            0x42 if self.outputs => self.output(f.payload, out),
            _ => Err(Error::Unsupported),
        }
    }
}
impl ServiceHandler for PersistentMaintenanceHandler<'_> {
    fn required_features(&self, e: &ModuleEntry, op: u8) -> Option<u32> {
        (*e == self.metadata && (matches!(op, 0x40 | 0x41) || (op == 0x42 && self.outputs)))
            .then_some(0)
    }
    fn authorized(&self, _: Frame<'_>) -> bool {
        if self.guard.closed() || !self.scope() {
            return false;
        }
        self.seen.set(true);
        true
    }
    fn must_end_session(&self) -> bool {
        if self.authority.station_id() != Some(self.station)
            || (self.seen.get() && !self.scope())
            || self
                .pending
                .get()
                .is_some_and(|(_, r)| !self.context_allowed(true, r))
            || (self.pending.get().is_some()
                && self.pending_danger.get()
                && !self.authority.allows(Capability::DangerousMaintenance))
        {
            self.close();
        }
        self.guard.closed()
    }
    fn replay_valid(&self, f: Frame<'_>, response: &[u8]) -> bool {
        if self.guard.closed() || response.len() < 4 {
            return false;
        }
        if response[..2] != [0, 0] {
            return true;
        }
        let Some((write, old)) = self.context.get() else {
            return false;
        };
        if !self.scope()
            || !self.context_allowed(write, old)
            || (write
                && self.context_danger.get()
                && !self.authority.allows(Capability::DangerousMaintenance))
        {
            return false;
        }
        let id = match f.header.opcode {
            0x40 => NamedRequest::decode(f.payload).map(|r| r.operation_id),
            0x41 | 0x42 => id16(f.payload),
            _ => return false,
        };
        let Ok(id) = id else { return false };
        self.engine.try_borrow_mut().is_ok_and(|mut e| {
            e.query(self.metadata.module, id).is_ok_and(|r| {
                r.operation_id == old.operation_id
                    && r.module == old.module
                    && r.operation == old.operation
                    && r.schema == old.schema
                    && r.job_id == old.job_id
                    && r.station_id == old.station_id
                    && r.policy_revision == old.policy_revision
            })
        })
    }
    fn response_admitted(&mut self, f: Frame<'_>, response: &[u8]) -> Result<(), SessionError> {
        if self.must_end_session() {
            return Err(SessionError::AuthorizationRevoked);
        }
        self.guard.now().map_err(crate::common::local)?;
        if f.header.opcode != 0x40 {
            return Ok(());
        }
        if self.last_admitted.get() == Some(f.header.message_id) {
            return Ok(());
        }
        let Some((request, r)) = self.pending.get() else {
            return Err(SessionError::InvalidHandlerResponse);
        };
        if request != f.header.message_id
            || response.len() != 8
            || response[..4] != [0, 0, 0, 0]
            || crate::common::u32_at(response, 4) != r.job_id
            || !self.replay_valid(f, response)
        {
            self.close();
            return Err(SessionError::AuthorizationRevoked);
        }
        if r.state == PersistentState::Accepted {
            let result = self
                .executor
                .try_borrow_mut()
                .map_err(|_| SessionError::InvalidHandlerResponse)?
                .response_admitted(r, self.authority);
            if let Err(e) = result {
                self.close();
                return Err(crate::common::local(Error::from(e)));
            }
        }
        self.pending.set(None);
        self.last_admitted.set(Some(f.header.message_id));
        Ok(())
    }
    fn connection_closed(&mut self) -> Result<(), SessionError> {
        self.close();
        Ok(())
    }
    fn dispatch(&mut self, f: Frame<'_>, out: &mut [u8]) -> Result<usize, SessionError> {
        frame(f, self.metadata)?;
        if self.must_end_session() {
            out.fill(0);
            return Err(SessionError::AuthorizationRevoked);
        }
        self.guard.now().map_err(crate::common::local)?;
        if out.len() < 4 {
            return Err(SessionError::Capacity);
        }
        if !self.authorized(f) {
            return Ok(status(out, Status::AccessDenied, Reason::RoleRequired));
        }
        let result = self.execute(f, out);
        if !self.authorized(f) || self.must_end_session() {
            out.fill(0);
            return Err(SessionError::AuthorizationRevoked);
        }
        match result {
            Ok(n) => Ok(n),
            Err(Error::Capacity) => Err(SessionError::Capacity),
            Err(Error::Engine(device_maintenance::Error::Backend)) => {
                Ok(failure(out, Error::Storage))
            }
            Err(e) => Ok(failure(out, e)),
        }
    }
}

fn validate_params(module: u8, r: NamedRequest<'_>) -> Result<(), Error> {
    use crate::common::{u16_at, u32_at};
    let p = r.params;
    if p.len() < 2 || p.len() > 492 || p[..2] != [1, 0] {
        return Err(Error::Invalid);
    }
    let n = match (module, r.operation) {
        (0x72, 1 | 2 | 8) | (0x73, 1 | 3) => 4,
        (0x72, 3 | 5 | 6 | 7) => 6,
        (0x72, 4) => 10,
        (0x72, 9) => 12,
        (0x72, 10 | 11) => 7,
        (0x72, 12) | (0x73, 5) => 3,
        (0x73, 4) => 5,
        (0x73, 2) => {
            if p.len() < 5 {
                return Err(Error::Invalid);
            }
            let n = usize::from(u16_at(p, 2));
            if !(1..=32).contains(&n) || p.len() != 4 + n || !p[4..].iter().all(u8::is_ascii_digit)
            {
                return Err(Error::Invalid);
            }
            return Ok(());
        }
        _ => return Err(Error::Invalid),
    };
    if p.len() != n {
        return Err(Error::Invalid);
    }
    let valid = match (module, r.operation) {
        (0x72, 4) => u32_at(p, 6) > 0,
        (0x72, 5 | 6) => u16_at(p, 4) > 0,
        (0x72, 9) => u32_at(p, 4) > 0 && u32_at(p, 8) > 0,
        (0x72, 10) => match p[2] {
            1 => u32_at(p, 3) > 0,
            2 => u32_at(p, 3) == 0,
            _ => false,
        },
        (0x72, 11) => matches!(p[2], 1 | 2) && u32_at(p, 3) > 0,
        (0x72, 12) => p[2] <= 2,
        (0x73, 4) => p[4] <= 1,
        (0x73, 5) => p[2] <= 1,
        _ => true,
    };
    if valid { Ok(()) } else { Err(Error::Invalid) }
}
