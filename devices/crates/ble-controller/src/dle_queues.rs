//! Long ACL owners are bounded independently of control and event queue depth.
use crate::{ControllerEvent, FrameKind, LinkFrame, OwnedQueue, QueueFull};

#[derive(Debug, Clone, Copy)]
enum TxEntry {
    Host,
    Control {
        bytes: [u8; 27],
        len: u8,
        terminate: bool,
    },
}

/// Borrowed wire bytes plus the action to take once the peer acknowledges them.
pub(super) struct TxFragment<'a> {
    pub kind: FrameKind,
    pub payload: &'a [u8],
    pub more: bool,
    pub completes_host: bool,
    pub terminate: bool,
}
impl<'a> TxFragment<'a> {
    pub const fn control(payload: &'a [u8], more: bool, terminate: bool) -> Self {
        Self {
            kind: FrameKind::Control,
            payload,
            more,
            completes_host: false,
            terminate,
        }
    }
}
#[derive(Debug)]
pub(super) struct TxQueue {
    order: OwnedQueue<TxEntry, 8>,
    hosts: OwnedQueue<LinkFrame, 4>,
    offset: u8,
}

impl TxQueue {
    pub const fn new() -> Self {
        Self {
            order: OwnedQueue::new(),
            hosts: OwnedQueue::new(),
            offset: 0,
        }
    }
    pub fn clear(&mut self) {
        self.order.clear();
        self.hosts.clear();
        self.offset = 0;
    }
    pub const fn is_full(&self) -> bool {
        self.order.is_full()
    }
    pub const fn is_empty(&self) -> bool {
        self.order.is_empty()
    }
    pub fn push_host(&mut self, frame: LinkFrame) -> Result<(), QueueFull> {
        if self.order.is_full() {
            return Err(QueueFull);
        }
        self.hosts.push(frame)?;
        self.order.push(TxEntry::Host)
    }
    pub fn push_control(&mut self, payload: &[u8], terminate: bool) -> Result<(), QueueFull> {
        let mut bytes = [0; 27];
        bytes[..payload.len()].copy_from_slice(payload);
        self.order.push(TxEntry::Control {
            bytes,
            len: u8::try_from(payload.len()).map_err(|_| QueueFull)?,
            terminate,
        })
    }
    pub fn next(&self, limit: u8) -> Option<TxFragment<'_>> {
        let another = self.order.len() > 1;
        match self.order.front()? {
            TxEntry::Control {
                bytes,
                len,
                terminate,
            } => Some(TxFragment::control(
                &bytes[..usize::from(*len)],
                another,
                *terminate,
            )),
            TxEntry::Host => {
                let host = self.hosts.front()?;
                let bytes = &host.payload()[usize::from(self.offset)..];
                let count = bytes.len().min(usize::from(limit.max(4)));
                let kind = if self.offset == 0 {
                    host.kind()
                } else {
                    FrameKind::Continuation
                };
                let complete = count == bytes.len();
                Some(TxFragment {
                    kind,
                    payload: &bytes[..count],
                    more: another || !complete,
                    completes_host: complete,
                    terminate: false,
                })
            }
        }
    }
    pub fn committed(&mut self, end_of_host: bool, consumed: u8) {
        if matches!(self.order.front(), Some(TxEntry::Host)) {
            if !end_of_host {
                self.offset += consumed;
                return;
            }
            self.hosts.pop();
        }
        self.order.pop();
        self.offset = 0;
    }
}

impl<F> ControllerEvent<F> {
    fn map_frame<G>(self, map: impl FnOnce(F) -> Option<G>) -> Option<ControllerEvent<G>> {
        Some(match self {
            Self::Acl { id, frame } => ControllerEvent::Acl {
                id,
                frame: map(frame)?,
            },
            Self::Completed { id, packets } => ControllerEvent::Completed { id, packets },
            Self::Connected {
                id,
                peer,
                parameters,
            } => ControllerEvent::Connected {
                id,
                peer,
                parameters,
            },
            Self::Disconnected { id, reason } => ControllerEvent::Disconnected { id, reason },
            Self::PhyUpdated { id, tx, rx } => ControllerEvent::PhyUpdated { id, tx, rx },
            Self::ParametersUpdated { id, parameters } => {
                ControllerEvent::ParametersUpdated { id, parameters }
            }
            Self::DataLengthChanged { id, limits } => {
                ControllerEvent::DataLengthChanged { id, limits }
            }
        })
    }
}

#[derive(Debug)]
pub(super) struct EventQueue {
    order: OwnedQueue<ControllerEvent<()>, 16>,
    received: OwnedQueue<LinkFrame, 2>,
}

impl EventQueue {
    pub const fn new() -> Self {
        Self {
            order: OwnedQueue::new(),
            received: OwnedQueue::new(),
        }
    }
    pub fn clear(&mut self) {
        self.order.clear();
        self.received.clear();
    }
    #[cfg(feature = "external-wake")]
    pub const fn is_empty(&self) -> bool {
        self.order.is_empty()
    }
    pub const fn is_full(&self) -> bool {
        self.order.is_full() || self.received.is_full()
    }
    pub fn push_acl(&mut self, id: crate::ConnectionId, frame: LinkFrame) -> Result<(), QueueFull> {
        if self.order.is_full() {
            return Err(QueueFull);
        }
        self.received.push(frame)?;
        self.order.push(ControllerEvent::Acl { id, frame: () })
    }
    pub fn push(&mut self, event: ControllerEvent<()>) -> Result<(), QueueFull> {
        self.order.push(event)
    }
    pub fn pop(&mut self) -> Option<ControllerEvent> {
        self.order.pop()?.map_frame(|()| self.received.pop())
    }
}
