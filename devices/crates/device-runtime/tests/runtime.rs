//! Offline transport fixtures; all wire/session/reassembly work uses the existing production actors.
#![allow(clippy::unwrap_used)]
use core::cell::Cell;
use device_connection::{ConnectionError, FirstFragmentQueue, FirstSlotError, FirstSlotOutcome};
use device_protocol::{Frame, ModuleEntry, ProtocolLimits};
use device_runtime::{
    Budgets, OwnedRx, PortError, Runtime, RuntimeError, Scope, SessionFactory, Transport, Work,
};
use device_services::{
    BuildProfile, Config, ConfigError, DeviceInfo, ServiceHandler, SessionError,
};
use std::collections::VecDeque;

const MODULES: [ModuleEntry; 2] = [
    ModuleEntry {
        module: 0,
        major: 1,
        minor: 2,
        availability: 1,
        features: 0,
        input_max: 128,
        output_max: 128,
    },
    ModuleEntry {
        module: 2,
        major: 1,
        minor: 2,
        availability: 1,
        features: 0,
        input_max: 128,
        output_max: 128,
    },
];
const HELLO: &[u8] = &[0x10, 0, 1, 0, 1, 0, 0, 0, 8, 0, 1, 1, 128, 0, 0, 0, 3, 0];
const ACTION: &[u8] = &[0x10, 2, 0x30, 0, 2, 0, 0, 0, 0, 0];
#[derive(Debug)]
struct Factory {
    sessions: Cell<u32>,
    calls: Cell<u32>,
    admitted: Cell<u32>,
    closes: Cell<u32>,
    fail_cleanup: Cell<bool>,
    allowed: Cell<bool>,
    deferred: Cell<bool>,
    short: Cell<bool>,
    fresh_id: Cell<bool>,
    fail_open: Cell<bool>,
    frame_max: Cell<u16>,
    data_max: Cell<u16>,
    capability_rev: Cell<u32>,
}
impl Factory {
    fn new() -> Self {
        Self {
            sessions: Cell::new(0),
            calls: Cell::new(0),
            admitted: Cell::new(0),
            closes: Cell::new(0),
            fail_cleanup: Cell::new(false),
            allowed: Cell::new(true),
            deferred: Cell::new(false),
            short: Cell::new(false),
            fresh_id: Cell::new(true),
            fail_open: Cell::new(false),
            frame_max: Cell::new(20),
            data_max: Cell::new(0),
            capability_rev: Cell::new(1),
        }
    }
}
#[derive(Debug)]
struct Handler<'a>(&'a Factory);
impl ServiceHandler for Handler<'_> {
    fn required_features(&self, entry: &ModuleEntry, op: u8) -> Option<u32> {
        (entry.module == 2 && op == 0x30).then_some(0)
    }
    fn authorized(&self, _: Frame<'_>) -> bool {
        self.0.allowed.get()
    }
    fn response_ready(&mut self, _: Frame<'_>) -> Result<bool, SessionError> {
        Ok(!self.0.deferred.get())
    }
    fn dispatch(&mut self, _: Frame<'_>, out: &mut [u8]) -> Result<usize, SessionError> {
        self.0.calls.set(self.0.calls.get() + 1);
        let len = if self.0.short.get() { 4 } else { 14 };
        out[..len].fill(0);
        Ok(len)
    }
    fn response_admitted(&mut self, _: Frame<'_>, _: &[u8]) -> Result<(), SessionError> {
        self.0.admitted.set(self.0.admitted.get() + 1);
        Ok(())
    }
    fn connection_closed(&mut self) -> Result<(), SessionError> {
        self.0.closes.set(self.0.closes.get() + 1);
        if self.0.fail_cleanup.get() {
            Err(SessionError::InvalidHandlerResponse)
        } else {
            Ok(())
        }
    }
}
impl SessionFactory for Factory {
    type Handler<'a> = Handler<'a>;
    fn open(&self, _: Scope) -> Result<(Config<'_>, Self::Handler<'_>), PortError> {
        if self.fail_open.get() {
            return Err(PortError::Unavailable);
        }
        if self.fresh_id.get() {
            self.sessions.set(self.sessions.get() + 1);
        }
        Ok((
            Config {
                session: self.sessions.get(),
                capability_rev: self.capability_rev.get(),
                profile: BuildProfile::Commercial,
                info: DeviceInfo {
                    product_id: 5,
                    fw_major: 1,
                    fw_minor: 0,
                    fw_patch: 0,
                    build_id: 1,
                },
                modules: &MODULES,
                limits: ProtocolLimits {
                    rx_frame_max: self.frame_max.get(),
                    tx_frame_max: self.frame_max.get(),
                    control_max: 128,
                    data_max: self.data_max.get(),
                    idle_ms: 2000,
                    fragment_ms: 100,
                    response_ms: 10000,
                },
            },
            Handler(self),
        ))
    }
}
#[derive(Debug, Default)]
struct Slice {
    calls: u32,
    fail: bool,
}
impl Work for Slice {
    fn step(&mut self, _: u64) -> Result<(), PortError> {
        self.calls += 1;
        if self.fail {
            Err(PortError::Fault)
        } else {
            Ok(())
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QueueMode {
    Ready,
    Full,
    Uncertain,
    ScopeChange,
}
#[derive(Debug)]
struct Wire {
    scope: Option<Scope>,
    rx: VecDeque<OwnedRx>,
    sent: Vec<Vec<u8>>,
    mode: QueueMode,
    on_receive_scope: Option<Scope>,
    poll_error: Option<PortError>,
    revoke: Option<&'static Cell<bool>>,
    fills: u32,
    polls: u32,
    popped: u32,
    close_fail: bool,
    revoke_after: Option<&'static Cell<bool>>,
}
impl Wire {
    fn new() -> Self {
        Self {
            scope: Some(scope(1)),
            rx: VecDeque::new(),
            sent: vec![],
            mode: QueueMode::Ready,
            on_receive_scope: None,
            poll_error: None,
            revoke: None,
            fills: 0,
            polls: 0,
            popped: 0,
            close_fail: false,
            revoke_after: None,
        }
    }
    fn push(&mut self, bytes: &[u8]) {
        self.rx
            .push_back(OwnedRx::new(self.scope.unwrap(), bytes).unwrap());
    }
}
#[derive(Debug)]
struct Queue<'a> {
    wire: &'a mut Wire,
    expected: Scope,
}
impl FirstFragmentQueue for Queue<'_> {
    fn with_reserved_slot<T, E>(
        &mut self,
        fill: impl FnOnce(&mut [u8]) -> Result<(usize, T), E>,
    ) -> Result<FirstSlotOutcome<T>, FirstSlotError<E>> {
        if self.wire.mode == QueueMode::Full {
            return Ok(FirstSlotOutcome::NoSlot);
        }
        if self.wire.mode == QueueMode::ScopeChange {
            self.wire.scope = Some(scope(2));
        }
        if self.wire.scope != Some(self.expected) {
            return Err(FirstSlotError::Contract);
        }
        if let Some(allowed) = self.wire.revoke {
            allowed.set(false);
        }
        let mut bytes = [0; 20];
        self.wire.fills += 1;
        let (len, value) = fill(&mut bytes).map_err(FirstSlotError::Fill)?;
        if self.wire.mode == QueueMode::Uncertain {
            return Err(FirstSlotError::Transport);
        }
        self.wire.sent.push(bytes[..len].to_vec());
        Ok(FirstSlotOutcome::Admitted(value))
    }
}
impl Transport for Wire {
    type Queue<'a> = Queue<'a>;
    fn poll(&mut self, _: u64, _: u8) -> Result<(), PortError> {
        self.polls += 1;
        self.poll_error.map_or(Ok(()), Err)
    }
    fn scope(&self) -> Result<Option<Scope>, PortError> {
        Ok(self.scope)
    }
    fn receive(&mut self) -> Result<Option<OwnedRx>, PortError> {
        let packet = self.rx.pop_front();
        if let Some(scope) = self.on_receive_scope.take() {
            self.scope = Some(scope);
        }
        if packet.is_some() {
            self.popped += 1;
        }
        Ok(packet)
    }
    fn queue(&mut self, expected: Scope) -> Result<Self::Queue<'_>, PortError> {
        Ok(Queue {
            wire: self,
            expected,
        })
    }
    fn close_scope(&mut self, old: Scope) -> Result<(), PortError> {
        if self.close_fail {
            return Err(PortError::Fault);
        }
        if self.scope == Some(old) {
            self.scope = None;
        }
        Ok(())
    }
}
fn scope(id: u64) -> Scope {
    Scope::new(id, 20).unwrap()
}
fn runtime(factory: &Factory) -> Runtime<'_, Factory> {
    Runtime::new(factory, Budgets::new(2, 2, 1).unwrap())
}
fn turn(runtime: &mut Runtime<'_, Factory>, now: u64, wire: &mut Wire) {
    let report = runtime.step(now, wire, &mut Slice::default(), &mut Slice::default());
    assert!(report.connection.is_ok(), "{report:?}");
}
fn hello(runtime: &mut Runtime<'_, Factory>, wire: &mut Wire) {
    wire.push(HELLO);
    turn(runtime, 0, wire);
    turn(runtime, 1, wire);
    wire.sent.clear();
}

#[test]
fn test_should_validate_scope_budget_and_owned_frame_boundaries() {
    assert!(Scope::new(0, 20).is_err());
    assert!(Scope::new(1, 19).is_err());
    assert!(Scope::new(1, 513).is_err());
    assert!(Budgets::new(0, 1, 1).is_err());
    assert!(Budgets::new(1, 0, 1).is_err());
    assert!(Budgets::new(1, 1, 0).is_err());
    assert!(OwnedRx::new(scope(1), &[0; 21]).is_err());
    assert!(OwnedRx::new(scope(1), &[]).is_err());
}
#[test]
fn test_should_retry_real_response_without_consuming_no_slot_or_busy_rx() {
    let f = Factory::new();
    let mut rt = runtime(&f);
    let mut wire = Wire::new();
    hello(&mut rt, &mut wire);
    wire.mode = QueueMode::Full;
    wire.push(ACTION);
    wire.push(ACTION);
    turn(&mut rt, 2, &mut wire);
    assert_eq!(f.calls.get(), 1);
    assert_eq!(f.admitted.get(), 0);
    let fills = wire.fills;
    turn(&mut rt, 3, &mut wire);
    assert_eq!(wire.fills, fills);
    wire.mode = QueueMode::Ready;
    turn(&mut rt, 4, &mut wire);
    turn(&mut rt, 5, &mut wire);
    turn(&mut rt, 6, &mut wire);
    assert_eq!(f.calls.get(), 1);
    assert_eq!(f.admitted.get(), 1);
    assert_eq!(wire.sent.len(), 4);
    assert_eq!(&wire.sent[0][..10], &[0x11, 2, 0x30, 0, 2, 0, 0, 0, 14, 0]);
    assert_eq!(wire.sent[0], wire.sent[2]);
    assert_eq!(wire.sent[1], wire.sent[3]);
}
#[test]
fn test_should_reconstruct_five_subscription_cycles_with_fresh_hello_ids_and_retained_owner() {
    let f = Factory::new();
    let mut rt = runtime(&f);
    let mut wire = Wire::new();
    for cycle in 1..=5 {
        wire.scope = Some(scope(cycle));
        wire.push(HELLO);
        turn(&mut rt, cycle * 10, &mut wire);
        turn(&mut rt, cycle * 10 + 1, &mut wire);
        wire.push(ACTION);
        turn(&mut rt, cycle * 10 + 2, &mut wire);
        wire.scope = None;
        turn(&mut rt, cycle * 10 + 3, &mut wire);
    }
    assert_eq!(f.sessions.get(), 5);
    assert_eq!(f.calls.get(), 5);
    assert_eq!(f.closes.get(), 5);
    let ids: Vec<_> = wire
        .sent
        .iter()
        .filter(|b| b[1] == 0 && b[2] == 1 && b[6] == 0)
        .map(|b| u32::from_le_bytes(b[16..20].try_into().unwrap()))
        .collect();
    assert_eq!(ids, vec![1, 2, 3, 4, 5]);
}
#[test]
fn test_should_keep_cleanup_failure_blocked_until_explicit_retry() {
    let f = Factory::new();
    let mut rt = runtime(&f);
    let mut wire = Wire::new();
    hello(&mut rt, &mut wire);
    f.fail_cleanup.set(true);
    wire.scope = None;
    let report = rt.step(2, &mut wire, &mut Slice::default(), &mut Slice::default());
    assert!(report.connection.is_err());
    wire.scope = Some(scope(2));
    assert_eq!(
        rt.step(3, &mut wire, &mut Slice::default(), &mut Slice::default())
            .connection,
        Err(RuntimeError::CleanupBlocked)
    );
    assert_eq!(f.sessions.get(), 1);
    f.fail_cleanup.set(false);
    rt.retry_cleanup(&mut wire).unwrap();
    turn(&mut rt, 4, &mut wire);
    assert_eq!(f.sessions.get(), 2);
}
#[test]
fn test_should_fail_closed_for_scope_change_during_queue_publication_and_uncertain_transport() {
    for flip in [true, false] {
        let f = Factory::new();
        let mut rt = runtime(&f);
        let mut wire = Wire::new();
        hello(&mut rt, &mut wire);
        wire.mode = if flip {
            QueueMode::ScopeChange
        } else {
            QueueMode::Uncertain
        };
        wire.push(ACTION);
        assert!(
            rt.step(2, &mut wire, &mut Slice::default(), &mut Slice::default())
                .connection
                .is_err()
        );
        assert!(wire.sent.is_empty());
        assert_eq!(f.admitted.get(), 0);
        assert_eq!(f.closes.get(), 1);
        wire.mode = QueueMode::Ready;
        if !flip {
            wire.scope = Some(scope(1));
        }
        let result = rt.step(3, &mut wire, &mut Slice::default(), &mut Slice::default());
        if !flip {
            assert_eq!(result.connection, Err(RuntimeError::StaleScope));
        }
    }
}
#[test]
fn test_should_reassemble_fragments_and_drop_stale_owned_rx() {
    let f = Factory::new();
    let mut rt = runtime(&f);
    let mut wire = Wire::new();
    wire.scope = Some(scope(2));
    hello(&mut rt, &mut wire);
    wire.rx.push_back(OwnedRx::new(scope(1), ACTION).unwrap());
    wire.push(&[
        0x10, 2, 0x30, 0, 2, 0, 0, 0, 14, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10,
    ]);
    wire.push(&[0x10, 2, 0x30, 0, 2, 0, 10, 0, 14, 0, 11, 12, 13, 14]);
    turn(&mut rt, 2, &mut wire);
    assert_eq!(f.calls.get(), 0);
    turn(&mut rt, 3, &mut wire);
    assert_eq!(f.calls.get(), 1);
    assert_eq!(f.admitted.get(), 1);
}
#[test]
fn test_should_run_ui_and_background_slices_even_when_ble_or_ui_fails_and_clock_regresses() {
    let f = Factory::new();
    let mut rt = runtime(&f);
    let mut wire = Wire::new();
    let mut ui = Slice {
        fail: true,
        ..Slice::default()
    };
    let mut bg = Slice::default();
    wire.push(&[0]);
    let report = rt.step(10, &mut wire, &mut ui, &mut bg);
    assert!(report.connection.is_err());
    assert_eq!(report.ui, Err(PortError::Fault));
    assert_eq!((ui.calls, bg.calls), (1, 1));
    wire.scope = Some(scope(2));
    let report = rt.step(9, &mut wire, &mut ui, &mut bg);
    assert_eq!(report.connection, Err(RuntimeError::ClockRegression));
    assert_eq!((ui.calls, bg.calls), (2, 2));
    assert_eq!(wire.polls, 2);
}

#[test]
fn test_should_not_retry_handler_cleanup_implicitly_after_receive_failure() {
    let f = Factory::new();
    let mut rt = runtime(&f);
    let mut wire = Wire::new();
    hello(&mut rt, &mut wire);
    f.fail_cleanup.set(true);
    wire.push(&[0]);
    let first = rt.step(2, &mut wire, &mut Slice::default(), &mut Slice::default());
    assert!(first.connection.is_err());
    assert_eq!(
        f.closes.get(),
        1,
        "one failed cleanup must be retained, not retried in same step"
    );
    f.fail_cleanup.set(false);
    wire.scope = Some(scope(2));
    assert_eq!(
        rt.step(3, &mut wire, &mut Slice::default(), &mut Slice::default())
            .connection,
        Err(RuntimeError::CleanupBlocked)
    );
    rt.retry_cleanup(&mut wire).unwrap();
    turn(&mut rt, 4, &mut wire);
    assert_eq!(f.sessions.get(), 2);
}
#[test]
fn test_should_keep_serialized_queue_guard_until_admission_callback() {
    let f = Box::leak(Box::new(Factory::new()));
    let mut rt = runtime(f);
    let mut wire = Wire::new();
    hello(&mut rt, &mut wire);
    f.short.set(true);
    wire.revoke_after = Some(&f.allowed);
    wire.push(ACTION);
    let report = rt.step(2, &mut wire, &mut Slice::default(), &mut Slice::default());
    assert!(report.connection.is_ok(), "{report:?}");
    assert_eq!(f.admitted.get(), 1);
}
impl Drop for Queue<'_> {
    fn drop(&mut self) {
        if let Some(allowed) = self.wire.revoke_after {
            allowed.set(false);
        }
    }
}
#[test]
fn test_should_recheck_handler_authorization_inside_real_queue_fill() {
    let f = Box::leak(Box::new(Factory::new()));
    let mut rt = runtime(f);
    let mut wire = Wire::new();
    hello(&mut rt, &mut wire);
    wire.revoke = Some(&f.allowed);
    wire.push(ACTION);
    let report = rt.step(2, &mut wire, &mut Slice::default(), &mut Slice::default());
    assert!(report.connection.is_err());
    assert!(wire.sent.is_empty());
    assert_eq!(f.calls.get(), 1);
    assert_eq!(f.admitted.get(), 0);
    assert_eq!(f.closes.get(), 1);
}
#[test]
fn test_should_not_dispatch_unauthorized_business_request() {
    let f = Factory::new();
    let mut rt = runtime(&f);
    let mut wire = Wire::new();
    hello(&mut rt, &mut wire);
    f.allowed.set(false);
    wire.push(ACTION);
    turn(&mut rt, 2, &mut wire);
    assert_eq!(f.calls.get(), 0);
    assert_eq!(&wire.sent[0][10..], &[5, 0, 3, 255]);
}
#[test]
fn test_should_poll_deferred_handler_and_service_all_lanes_under_rx_flood() {
    let f = Factory::new();
    let mut rt = runtime(&f);
    let mut wire = Wire::new();
    hello(&mut rt, &mut wire);
    f.deferred.set(true);
    wire.push(ACTION);
    for _ in 0..30 {
        wire.push(ACTION);
    }
    let mut ui = Slice::default();
    let mut bg = Slice::default();
    let before = wire.popped;
    for now in 2..7 {
        let report = rt.step(now, &mut wire, &mut ui, &mut bg);
        assert!(report.connection.is_ok());
    }
    assert!(wire.popped - before <= 2);
    assert_eq!(f.calls.get(), 0);
    assert_eq!((ui.calls, bg.calls), (5, 5));
    f.deferred.set(false);
    turn(&mut rt, 7, &mut wire);
    turn(&mut rt, 8, &mut wire);
    assert_eq!(f.calls.get(), 1);
    assert_eq!(f.admitted.get(), 1);
}
#[test]
fn test_should_block_transport_cleanup_uncertainty_and_keep_healthy_lanes_running() {
    let f = Factory::new();
    let mut rt = runtime(&f);
    let mut wire = Wire::new();
    hello(&mut rt, &mut wire);
    wire.close_fail = true;
    wire.scope = None;
    let mut ui = Slice::default();
    let mut bg = Slice::default();
    assert!(rt.step(2, &mut wire, &mut ui, &mut bg).connection.is_err());
    wire.scope = Some(scope(2));
    assert_eq!(
        rt.step(3, &mut wire, &mut ui, &mut bg).connection,
        Err(RuntimeError::CleanupBlocked)
    );
    assert_eq!(f.sessions.get(), 1);
    assert_eq!((ui.calls, bg.calls), (2, 2));
    wire.close_fail = false;
    rt.retry_cleanup(&mut wire).unwrap();
    turn(&mut rt, 4, &mut wire);
    assert_eq!(f.sessions.get(), 2);
}

#[test]
fn test_should_recheck_actual_scope_after_taking_rx_ownership_before_dispatch() {
    let f = Factory::new();
    let mut rt = runtime(&f);
    let mut wire = Wire::new();
    hello(&mut rt, &mut wire);
    wire.push(ACTION);
    wire.on_receive_scope = Some(scope(2));
    assert!(
        rt.step(2, &mut wire, &mut Slice::default(), &mut Slice::default())
            .connection
            .is_err()
    );
    assert_eq!(f.calls.get(), 0);
    assert_eq!(f.closes.get(), 1);
}
#[test]
fn test_should_reject_factory_session_reuse_and_unbound_data_and_actual_frame_overclaim() {
    for invalid in 0..3 {
        let f = Factory::new();
        let mut rt = runtime(&f);
        let mut wire = Wire::new();
        hello(&mut rt, &mut wire);
        wire.scope = None;
        turn(&mut rt, 2, &mut wire);
        wire.scope = Some(scope(2));
        match invalid {
            0 => f.fresh_id.set(false),
            1 => f.data_max.set(128),
            _ => f.frame_max.set(21),
        }
        assert_eq!(
            rt.step(3, &mut wire, &mut Slice::default(), &mut Slice::default())
                .connection,
            Err(RuntimeError::InvalidConfiguration)
        );
        assert_eq!(f.closes.get(), 2);
    }
}
#[test]
fn test_should_keep_repeated_enable_idempotent_and_drain_bounded_tx() {
    let f = Factory::new();
    let mut rt = Runtime::new(&f, Budgets::new(1, 1, 1).unwrap());
    let mut wire = Wire::new();
    wire.push(HELLO);
    let first = rt.step(0, &mut wire, &mut Slice::default(), &mut Slice::default());
    assert_eq!(first.connection.unwrap().transmitted, 1);
    for now in 1..6 {
        let progress = rt
            .step(now, &mut wire, &mut Slice::default(), &mut Slice::default())
            .connection
            .unwrap();
        assert!(progress.transmitted <= 1);
    }
    assert_eq!(f.sessions.get(), 1);
    assert_eq!(wire.sent.len(), 3);
    assert_eq!(f.closes.get(), 0);
}
#[test]
fn test_should_surface_host_and_background_failures_without_starving_ui() {
    let f = Factory::new();
    let mut rt = runtime(&f);
    let mut wire = Wire::new();
    hello(&mut rt, &mut wire);
    wire.poll_error = Some(PortError::Unavailable);
    let mut ui = Slice::default();
    let mut bg = Slice {
        fail: true,
        ..Slice::default()
    };
    let report = rt.step(2, &mut wire, &mut ui, &mut bg);
    assert_eq!(
        report.connection,
        Err(RuntimeError::Port(PortError::Unavailable))
    );
    assert_eq!(report.background, Err(PortError::Fault));
    assert_eq!((ui.calls, bg.calls), (1, 1));
    assert_eq!(f.closes.get(), 1);
}
#[test]
fn test_should_redact_owned_payload_and_shutdown_prevent_scope_reuse() {
    let packet = OwnedRx::new(scope(1), b"PRIVATE").unwrap();
    assert!(!format!("{packet:?}").contains("PRIVATE"));
    let f = Factory::new();
    let mut rt = runtime(&f);
    let mut wire = Wire::new();
    hello(&mut rt, &mut wire);
    rt.shutdown(&mut wire).unwrap();
    rt.shutdown(&mut wire).unwrap();
    assert_eq!(f.closes.get(), 1);
    wire.scope = Some(scope(1));
    assert_eq!(
        rt.step(2, &mut wire, &mut Slice::default(), &mut Slice::default())
            .connection,
        Err(RuntimeError::StaleScope)
    );
}

#[test]
fn test_should_retain_unbound_scope_cleanup_after_factory_open_failure() {
    let f = Factory::new();
    f.fail_open.set(true);
    let mut rt = runtime(&f);
    let mut wire = Wire::new();
    wire.close_fail = true;
    assert!(
        rt.step(0, &mut wire, &mut Slice::default(), &mut Slice::default())
            .connection
            .is_err()
    );
    f.fail_open.set(false);
    wire.scope = Some(scope(2));
    assert_eq!(
        rt.step(1, &mut wire, &mut Slice::default(), &mut Slice::default())
            .connection,
        Err(RuntimeError::CleanupBlocked)
    );
    assert_eq!(f.sessions.get(), 0);
    wire.close_fail = false;
    rt.retry_cleanup(&mut wire).unwrap();
    turn(&mut rt, 2, &mut wire);
    assert_eq!(f.sessions.get(), 1);
}
#[test]
fn test_should_close_silent_fragment_timeout_without_starving_slices() {
    let f = Factory::new();
    let mut rt = runtime(&f);
    let mut wire = Wire::new();
    hello(&mut rt, &mut wire);
    wire.push(&[
        0x10, 2, 0x30, 0, 2, 0, 0, 0, 14, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10,
    ]);
    turn(&mut rt, 2, &mut wire);
    let mut ui = Slice::default();
    let mut bg = Slice::default();
    assert!(
        rt.step(2003, &mut wire, &mut ui, &mut bg)
            .connection
            .is_err()
    );
    assert_eq!(f.calls.get(), 0);
    assert_eq!(f.closes.get(), 1);
    assert_eq!((ui.calls, bg.calls), (1, 1));
}

#[test]
fn test_should_retain_handler_after_constructor_rejection_until_explicit_cleanup() {
    for (handler_fails, transport_fails) in
        [(false, false), (true, false), (false, true), (true, true)]
    {
        let f = Factory::new();
        f.capability_rev.set(0);
        f.fail_cleanup.set(handler_fails);
        let mut rt = runtime(&f);
        let mut wire = Wire::new();
        wire.close_fail = transport_fails;
        wire.push(HELLO);
        let mut ui = Slice::default();
        let mut bg = Slice::default();
        let report = rt.step(0, &mut wire, &mut ui, &mut bg);
        let expected = if handler_fails {
            RuntimeError::Connection(ConnectionError::CleanupFailed(
                SessionError::InvalidHandlerResponse,
            ))
        } else if transport_fails {
            RuntimeError::Port(PortError::Fault)
        } else {
            RuntimeError::Connection(ConnectionError::Config(
                ConfigError::InvalidCapabilityRevision,
            ))
        };
        assert_eq!(report.connection, Err(expected));
        assert_eq!((f.sessions.get(), f.closes.get()), (1, 1));
        assert_eq!((f.calls.get(), f.admitted.get(), wire.popped), (0, 0, 0));
        assert!(wire.sent.is_empty());
        assert_eq!((ui.calls, bg.calls, wire.polls), (1, 1, 1));

        f.capability_rev.set(1);
        wire.scope = Some(scope(2));
        if handler_fails || transport_fails {
            assert_eq!(
                rt.step(1, &mut wire, &mut ui, &mut bg).connection,
                Err(RuntimeError::CleanupBlocked)
            );
            assert_eq!((f.sessions.get(), f.closes.get()), (1, 1));
            f.fail_cleanup.set(false);
            wire.close_fail = false;
            rt.retry_cleanup(&mut wire).unwrap();
            assert_eq!(f.closes.get(), 2);
            assert_eq!(wire.scope, Some(scope(2)));
        }
        // The rejected constructor consumed its generation and session identity.
        wire.scope = Some(scope(1));
        assert_eq!(
            rt.step(2, &mut wire, &mut ui, &mut bg).connection,
            Err(RuntimeError::StaleScope)
        );
        assert_eq!(f.sessions.get(), 1);
        wire.scope = Some(scope(2));
        f.fresh_id.set(false);
        assert_eq!(
            rt.step(3, &mut wire, &mut ui, &mut bg).connection,
            Err(RuntimeError::InvalidConfiguration)
        );
        assert_eq!(f.sessions.get(), 1);
        wire.scope = Some(scope(3));
        f.fresh_id.set(true);
        turn(&mut rt, 4, &mut wire);
        assert_eq!(f.sessions.get(), 2);
        assert_eq!(f.calls.get(), 0);
        assert_eq!(f.admitted.get(), 0);
        assert!(wire.sent.is_empty());
        rt.shutdown(&mut wire).unwrap();
    }
}
