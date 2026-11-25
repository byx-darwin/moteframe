//! Serialized bt-hci adapter with cooperative foreground or board-driven service.
#[cfg(feature = "external-wake")]
use crate::ConnectionId;
use crate::{
    AdvertisingData, AdvertisingParameters, CommandReply, Controller, ControllerError, FrameKind,
    HciBuffer, LinkBackend, LinkFrame, RandomAddress,
};
use bt_hci::{
    ControllerToHostPacket, FromHciBytes, WriteHci,
    cmd::{self, AsyncCmd, SyncCmd},
    controller::{Controller as Hci, ControllerCmdAsync, ControllerCmdSync},
    data::{AclBroadcastFlag, AclPacket, AclPacketBoundary, IsoPacket, SyncPacket},
};
#[cfg(feature = "external-wake")]
use core::future::Future;
use core::{
    cell::{Cell, RefCell},
    future::poll_fn,
    task::{Poll, Waker},
};
/// HCI adapter error, preserving the underlying controller failure.
#[derive(Debug)]
pub enum HciError<E> {
    /// Controller or backend failed.
    Controller(ControllerError<E>),
    /// Invalid ACL framing or inconsistent encoded data.
    InvalidPacket,
    /// A second reader, a second externally driven writer, or reentrant access was attempted.
    Busy,
    /// Synchronous/isochronous transport is not supported by this profile.
    Unsupported,
}
impl<E: core::fmt::Debug> core::fmt::Display for HciError<E> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl<E: core::fmt::Debug> core::error::Error for HciError<E> {}
impl<E: core::fmt::Debug> embedded_io::Error for HciError<E> {
    fn kind(&self) -> embedded_io::ErrorKind {
        embedded_io::ErrorKind::Other
    }
}
#[derive(Debug)]
pub(crate) struct HciState<B: LinkBackend> {
    pub core: Controller<B>,
    pub address: Option<RandomAddress>,
    pub parameters: Option<AdvertisingParameters>,
    pub data: Option<AdvertisingData>,
    pub scan: Option<AdvertisingData>,
    pub event_mask: u64,
    pub le_mask: u64,
}
fn register_waker(slot: &RefCell<Option<Waker>>, waker: &Waker) {
    // Clone/drop callbacks may reenter the adapter. Both run outside the borrow.
    let next = waker.clone();
    let previous = slot.borrow_mut().replace(next);
    drop(previous);
}
struct ReadLease<'a>(
    &'a Cell<bool>,
    #[cfg(feature = "external-wake")] Option<&'a RefCell<Option<Waker>>>,
);
impl Drop for ReadLease<'_> {
    fn drop(&mut self) {
        #[cfg(feature = "external-wake")]
        let waker = self.1.and_then(|slot| slot.borrow_mut().take());
        self.0.set(false);
        #[cfg(feature = "external-wake")]
        drop(waker);
    }
}

#[cfg(feature = "external-wake")]
struct WriteLease<'a, B: LinkBackend> {
    hci: &'a HciController<B>,
    id: ConnectionId,
}
#[cfg(feature = "external-wake")]
impl<B: LinkBackend> WriteLease<'_, B> {
    fn register(&self, waker: &Waker) {
        let next = waker.clone();
        if self.hci.writing.get() == Some(self.id) {
            let previous = self.hci.write_waker.borrow_mut().replace(next);
            drop(previous);
        }
    }
}
#[cfg(feature = "external-wake")]
impl<B: LinkBackend> Drop for WriteLease<'_, B> {
    fn drop(&mut self) {
        if self.hci.writing.get() == Some(self.id) {
            self.hci.writing.set(None);
            let waker = self.hci.write_waker.borrow_mut().take();
            drop(waker);
        }
    }
}

#[cfg(feature = "external-wake")]
struct Readiness {
    events: bool,
    acl_room: bool,
    faulted: bool,
    connection: Option<ConnectionId>,
}
#[cfg(feature = "external-wake")]
impl Readiness {
    fn of<B: LinkBackend>(core: &Controller<B>) -> Self {
        Self {
            events: core.has_events(),
            acl_room: core.has_acl_room(),
            faulted: core.faulted(),
            connection: core.connection(),
        }
    }
}
/// Local HCI transport with static bounded storage and one cancellable reader.
///
/// The transport cannot move between executors/threads, even for a Send backend:
/// ```compile_fail
/// fn require_send<B: ble_controller::LinkBackend + Send>() {
///     fn assert_send<T: Send>() {}
///     assert_send::<ble_controller::HciController<B>>();
/// }
/// ```
///
/// Keep this on a single cooperative executor. [`Self::new`] polls the backend
/// from `read` and wakes idle futures for foreground service.
/// The `external-wake` feature adds support for board-driven radio service.
/// No internal borrow survives an await or wake.
#[derive(Debug)]
pub struct HciController<B: LinkBackend> {
    local: core::marker::PhantomData<*mut ()>,
    state: RefCell<HciState<B>>,
    #[cfg(feature = "external-wake")]
    external_wake: bool,
    reading: Cell<bool>,
    #[cfg(feature = "external-wake")]
    writing: Cell<Option<ConnectionId>>,
    read_waker: RefCell<Option<Waker>>,
    write_waker: RefCell<Option<Waker>>,
}
impl<B: LinkBackend> HciController<B> {
    /// Wraps a Controller; the Host issues the standard Reset command.
    #[must_use]
    pub const fn new(core: Controller<B>) -> Self {
        Self {
            local: core::marker::PhantomData,
            state: RefCell::new(HciState {
                core,
                address: None,
                parameters: None,
                data: None,
                scan: None,
                event_mask: 0x0000_1fff_ffff_ffff,
                le_mask: 1,
            }),
            #[cfg(feature = "external-wake")]
            external_wake: false,
            reading: Cell::new(false),
            #[cfg(feature = "external-wake")]
            writing: Cell::new(None),
            read_waker: RefCell::new(None),
            write_waker: RefCell::new(None),
        }
    }

    /// Wraps a Controller for a board that independently schedules [`Self::service`].
    ///
    /// The Host still issues Reset. Calls to `read` drain staged packets, and a
    /// backpressured ACL write waits for transport progress. Neither self-wakes
    /// while idle. This mode permits a single outstanding ACL write future per
    /// connection. A disconnect or Reset invalidates the old writer.
    #[cfg(feature = "external-wake")]
    #[must_use]
    pub const fn new_with_external_wake(core: Controller<B>) -> Self {
        let mut hci = Self::new(core);
        hci.external_wake = true;
        hci
    }

    fn wake(&self) {
        let r = self.read_waker.borrow_mut().take();
        let w = self.write_waker.borrow_mut().take();
        if let Some(w) = r {
            w.wake();
        }
        if let Some(w) = w {
            w.wake();
        }
    }

    #[cfg(feature = "external-wake")]
    #[inline]
    fn update_external<R>(
        &self,
        op: impl FnOnce(&mut HciState<B>) -> R,
    ) -> Result<R, HciError<B::Error>> {
        let mut state = self.state.try_borrow_mut().map_err(|_| HciError::Busy)?;
        let before = Readiness::of(&state.core);
        let result = op(&mut state);
        let after = Readiness::of(&state.core);
        drop(state);

        let faulted = !before.faulted && after.faulted;
        let connection_changed = before.connection != after.connection;
        if connection_changed {
            self.writing.set(None);
        }
        let read = faulted || (!before.events && after.events);
        let write = faulted || (!before.acl_room && after.acl_room) || connection_changed;
        let r = read.then(|| self.read_waker.borrow_mut().take()).flatten();
        let w = write
            .then(|| self.write_waker.borrow_mut().take())
            .flatten();
        if let Some(w) = r {
            w.wake();
        }
        if let Some(w) = w {
            w.wake();
        }
        Ok(result)
    }

    /// Borrows the transport for a Host that takes ownership of its HCI endpoint.
    #[cfg(feature = "external-wake")]
    #[must_use]
    pub const fn borrowed(&self) -> HciControllerRef<'_, B> {
        HciControllerRef(self)
    }
    /// Executes one serialized raw standard command, returning bytes without status.
    /// # Errors
    /// Returns genuine HCI validation/unsupported errors or backend/access errors.
    #[allow(
        clippy::inline_always,
        reason = "Specialize fixed HCI opcodes without retaining the universal command frame"
    )]
    #[inline(always)]
    pub fn command(
        &self,
        opcode: u16,
        parameters: &[u8],
    ) -> Result<CommandReply, cmd::Error<HciError<B::Error>>> {
        #[cfg(feature = "external-wake")]
        if self.external_wake {
            return self
                .update_external(|state| state.command(opcode, parameters))
                .map_err(cmd::Error::Io)?;
        }
        let result = self
            .state
            .try_borrow_mut()
            .map_err(|_| cmd::Error::Io(HciError::Busy))?
            .command(opcode, parameters);
        self.wake();
        result
    }
    /// Advances bounded radio work and wakes pending transport futures.
    /// In external wake mode, service waits for the Host's first Reset and
    /// only wakes futures when their readiness changes.
    /// # Errors
    /// Returns reentrant access or a controller failure.
    pub fn service(&self) -> Result<(), HciError<B::Error>> {
        #[cfg(feature = "external-wake")]
        if self.external_wake {
            return self.update_external(|state| {
                if !state.core.initialized() {
                    return Ok(());
                }
                state.core.service().map_err(HciError::Controller)
            })?;
        }
        let result = self
            .state
            .try_borrow_mut()
            .map_err(|_| HciError::Busy)?
            .core
            .service()
            .map_err(HciError::Controller);
        self.wake();
        result
    }
    fn encode_command<C: cmd::Cmd + ?Sized>(
        &self,
        cmd: &C,
    ) -> Result<CommandReply, cmd::Error<HciError<B::Error>>> {
        let mut bytes = [0; 64];
        let len = cmd.params().size();
        if len > bytes.len() {
            return Err(bt_hci::param::Error::UNSUPPORTED.into());
        }
        cmd.params()
            .write_hci(&mut &mut bytes[..len])
            .map_err(|_| cmd::Error::Io(HciError::InvalidPacket))?;
        self.command(C::OPCODE.to_raw(), &bytes[..len])
    }
}
impl<B: LinkBackend> embedded_io::ErrorType for HciController<B> {
    type Error = HciError<B::Error>;
}
impl<B: LinkBackend> Hci for HciController<B> {
    type Buffer<'a> = HciBuffer;
    fn alloc_buf(&self) -> Result<HciBuffer, Self::Error> {
        Ok(HciBuffer::new())
    }
    async fn write_acl_data(&self, p: &AclPacket<'_>) -> Result<(), Self::Error> {
        if p.broadcast_flag() != AclBroadcastFlag::PointToPoint {
            return Err(HciError::InvalidPacket);
        }
        let kind = match p.boundary_flag() {
            AclPacketBoundary::FirstNonFlushable => FrameKind::Start,
            AclPacketBoundary::Continuing => FrameKind::Continuation,
            _ => return Err(HciError::InvalidPacket),
        };
        let frame = LinkFrame::new(kind, p.data()).map_err(|_| HciError::InvalidPacket)?;
        let id = self
            .state
            .try_borrow()
            .map_err(|_| HciError::Busy)?
            .core
            .connection()
            .filter(|id| id.handle() == p.handle().into_inner())
            .ok_or(HciError::InvalidPacket)?;
        #[cfg(feature = "external-wake")]
        let lease = if self.external_wake {
            if self.writing.get().is_some() {
                return Err(HciError::Busy);
            }
            self.writing.set(Some(id));
            Some(WriteLease { hci: self, id })
        } else {
            None
        };
        poll_fn(|cx| {
            #[cfg(feature = "external-wake")]
            if let Some(lease) = &lease {
                lease.register(cx.waker());
            }
            let result = {
                let Ok(mut s) = self.state.try_borrow_mut() else {
                    return Poll::Ready(Err(HciError::Busy));
                };
                s.core.send_acl(id, frame)
            };
            match result {
                Ok(()) => {
                    #[cfg(feature = "external-wake")]
                    if self.external_wake {
                        // The shared owner must admit the new ACL before the
                        // next scheduled radio slot closes the TX window.
                        cx.waker().wake_by_ref();
                        return Poll::Ready(Ok(()));
                    }
                    self.wake();
                    Poll::Ready(Ok(()))
                }
                Err(ControllerError::Capacity) => {
                    #[cfg(feature = "external-wake")]
                    if self.external_wake {
                        return Poll::Pending;
                    }
                    register_waker(&self.write_waker, cx.waker());
                    cx.waker().wake_by_ref();
                    Poll::Pending
                }
                Err(e) => Poll::Ready(Err(HciError::Controller(e))),
            }
        })
        .await
    }
    async fn write_sync_data(&self, _: &SyncPacket<'_>) -> Result<(), Self::Error> {
        Err(HciError::Unsupported)
    }
    async fn write_iso_data(&self, _: &IsoPacket<'_>) -> Result<(), Self::Error> {
        Err(HciError::Unsupported)
    }
    async fn read<'a>(
        &self,
        buf: &'a mut Self::Buffer<'_>,
    ) -> Result<ControllerToHostPacket<'a>, Self::Error> {
        if self.reading.replace(true) {
            return Err(HciError::Busy);
        }
        let _lease = ReadLease(
            &self.reading,
            #[cfg(feature = "external-wake")]
            self.external_wake.then_some(&self.read_waker),
        );
        poll_fn(|cx| {
            #[cfg(feature = "external-wake")]
            if self.external_wake {
                register_waker(&self.read_waker, cx.waker());
            }
            let result = (|| {
                let mut state = self.state.try_borrow_mut().map_err(|_| HciError::Busy)?;
                if !state.core.initialized() {
                    return Ok(false);
                }
                #[cfg(feature = "external-wake")]
                {
                    if self.external_wake {
                        state.core.check().map_err(HciError::Controller)?;
                    } else {
                        state.core.service().map_err(HciError::Controller)?;
                    }
                }
                #[cfg(not(feature = "external-wake"))]
                state.core.service().map_err(HciError::Controller)?;
                for _ in 0..16 {
                    let Some(event) = state.core.next_event() else {
                        break;
                    };
                    if buf.encode(event, state.event_mask, state.le_mask) {
                        return Ok(true);
                    }
                }
                Ok(false)
            })();
            match result {
                Ok(true) => {
                    #[cfg(feature = "external-wake")]
                    if self.external_wake {
                        return Poll::Ready(Ok(()));
                    }
                    self.wake();
                    Poll::Ready(Ok(()))
                }
                Ok(false) => {
                    #[cfg(feature = "external-wake")]
                    if self.external_wake {
                        return Poll::Pending;
                    }
                    register_waker(&self.read_waker, cx.waker());
                    cx.waker().wake_by_ref();
                    Poll::Pending
                }
                Err(e) => Poll::Ready(Err(e)),
            }
        })
        .await?;
        ControllerToHostPacket::from_hci_bytes_complete(&buf.bytes[..buf.len])
            .map_err(|_| HciError::InvalidPacket)
    }
}
impl<B: LinkBackend, C: SyncCmd + ?Sized> ControllerCmdSync<C> for HciController<B> {
    async fn exec(&self, cmd: &C) -> Result<C::Return, cmd::Error<Self::Error>> {
        let reply = self.encode_command(cmd)?;
        C::Return::from_hci_bytes_complete(reply.as_slice())
            .map_err(|_| cmd::Error::Io(HciError::InvalidPacket))
    }
}
impl<B: LinkBackend, C: AsyncCmd + ?Sized> ControllerCmdAsync<C> for HciController<B> {
    async fn exec(&self, cmd: &C) -> Result<(), cmd::Error<Self::Error>> {
        self.encode_command(cmd).map(|_| ())
    }
}

/// Borrowed bt-hci endpoint sharing the original controller's state and waiters.
#[cfg(feature = "external-wake")]
#[derive(Debug)]
pub struct HciControllerRef<'a, B: LinkBackend>(&'a HciController<B>);

#[cfg(feature = "external-wake")]
impl<B: LinkBackend> Copy for HciControllerRef<'_, B> {}
#[cfg(feature = "external-wake")]
impl<B: LinkBackend> Clone for HciControllerRef<'_, B> {
    fn clone(&self) -> Self {
        *self
    }
}
#[cfg(feature = "external-wake")]
impl<B: LinkBackend> embedded_io::ErrorType for HciControllerRef<'_, B> {
    type Error = HciError<B::Error>;
}
#[cfg(feature = "external-wake")]
impl<B: LinkBackend> Hci for HciControllerRef<'_, B> {
    type Buffer<'a> = HciBuffer;

    fn alloc_buf(&self) -> Result<HciBuffer, Self::Error> {
        self.0.alloc_buf()
    }
    fn write_acl_data(&self, p: &AclPacket<'_>) -> impl Future<Output = Result<(), Self::Error>> {
        self.0.write_acl_data(p)
    }
    fn write_sync_data(&self, p: &SyncPacket<'_>) -> impl Future<Output = Result<(), Self::Error>> {
        self.0.write_sync_data(p)
    }
    fn write_iso_data(&self, p: &IsoPacket<'_>) -> impl Future<Output = Result<(), Self::Error>> {
        self.0.write_iso_data(p)
    }
    fn read<'a>(
        &self,
        buf: &'a mut Self::Buffer<'_>,
    ) -> impl Future<Output = Result<ControllerToHostPacket<'a>, Self::Error>> {
        self.0.read(buf)
    }
}
#[cfg(feature = "external-wake")]
impl<B: LinkBackend, C: SyncCmd + ?Sized> ControllerCmdSync<C> for HciControllerRef<'_, B> {
    fn exec(&self, cmd: &C) -> impl Future<Output = Result<C::Return, cmd::Error<Self::Error>>> {
        ControllerCmdSync::exec(self.0, cmd)
    }
}
#[cfg(feature = "external-wake")]
impl<B: LinkBackend, C: AsyncCmd + ?Sized> ControllerCmdAsync<C> for HciControllerRef<'_, B> {
    fn exec(&self, cmd: &C) -> impl Future<Output = Result<(), cmd::Error<Self::Error>>> {
        ControllerCmdAsync::exec(self.0, cmd)
    }
}
