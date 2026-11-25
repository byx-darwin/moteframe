//! Serialized bt-hci adapter; foreground polling is cooperative, not low power.
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
    /// A second reader or reentrant access was attempted.
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
struct ReadLease<'a>(&'a Cell<bool>);
impl Drop for ReadLease<'_> {
    fn drop(&mut self) {
        self.0.set(false);
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
/// Keep this on a single cooperative executor. `read` polls the backend and
/// wakes itself while idle, so a foreground radio does not require an IRQ.
/// This deliberately busy-polls; no sleep/power claim is implied. Board code
/// can additionally call `service`. No internal borrow survives an await/wake.
#[derive(Debug)]
pub struct HciController<B: LinkBackend> {
    local: core::marker::PhantomData<*mut ()>,
    state: RefCell<HciState<B>>,
    reading: Cell<bool>,
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
            reading: Cell::new(false),
            read_waker: RefCell::new(None),
            write_waker: RefCell::new(None),
        }
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
        let result = self
            .state
            .try_borrow_mut()
            .map_err(|_| cmd::Error::Io(HciError::Busy))?
            .command(opcode, parameters);
        self.wake();
        result
    }
    /// Advances bounded radio work and wakes pending transport futures.
    /// # Errors
    /// Returns reentrant access or a controller failure.
    pub fn service(&self) -> Result<(), HciError<B::Error>> {
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
        poll_fn(|cx| {
            let result = {
                let Ok(mut s) = self.state.try_borrow_mut() else {
                    return Poll::Ready(Err(HciError::Busy));
                };
                s.core.send_acl(id, frame)
            };
            match result {
                Ok(()) => {
                    self.wake();
                    Poll::Ready(Ok(()))
                }
                Err(ControllerError::Capacity) => {
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
        let _lease = ReadLease(&self.reading);
        poll_fn(|cx| {
            let result = (|| {
                let mut state = self.state.try_borrow_mut().map_err(|_| HciError::Busy)?;
                if !state.core.initialized() {
                    return Ok(false);
                }
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
                    self.wake();
                    Poll::Ready(Ok(()))
                }
                Ok(false) => {
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
