use device_connection::{FirstFragmentQueue, FirstSlotError, FirstSlotOutcome};
use device_runtime::{OwnedRx, PortError, Scope, Transport, Work};
use std::collections::VecDeque;
#[derive(Debug)]
pub struct Wire {
    pub scope: Option<Scope>,
    pub rx: VecDeque<OwnedRx>,
    pub sent: Vec<Vec<u8>>,
}
impl Wire {
    pub fn new() -> Self {
        Self {
            scope: Some(Scope::new(1, 128).unwrap()),
            rx: VecDeque::new(),
            sent: Vec::new(),
        }
    }
    pub fn request(&mut self, module: u8, opcode: u8, id: u16, payload: &[u8]) {
        let mut bytes = vec![0x10, module, opcode, 0];
        bytes.extend_from_slice(&id.to_le_bytes());
        bytes.extend_from_slice(&[0, 0]);
        bytes.extend_from_slice(&u16::try_from(payload.len()).unwrap().to_le_bytes());
        bytes.extend_from_slice(payload);
        self.rx
            .push_back(OwnedRx::new(self.scope.unwrap(), &bytes).unwrap());
    }
    pub fn response(&self) -> device_protocol::Frame<'_> {
        device_protocol::Frame::decode(self.sent.last().unwrap(), 128, 128).unwrap()
    }
}
#[derive(Debug)]
pub struct Queue<'a>(&'a mut Wire, Scope);
impl FirstFragmentQueue for Queue<'_> {
    fn with_reserved_slot<T, E>(
        &mut self,
        fill: impl FnOnce(&mut [u8]) -> Result<(usize, T), E>,
    ) -> Result<FirstSlotOutcome<T>, FirstSlotError<E>> {
        if self.0.scope != Some(self.1) {
            return Err(FirstSlotError::Contract);
        }
        let mut bytes = [0; 128];
        let (n, result) = fill(&mut bytes).map_err(FirstSlotError::Fill)?;
        self.0.sent.push(bytes[..n].to_vec());
        Ok(FirstSlotOutcome::Admitted(result))
    }
}
impl Transport for Wire {
    type Queue<'a> = Queue<'a>;
    fn poll(&mut self, _: u64, _: u8) -> Result<(), PortError> {
        Ok(())
    }
    fn scope(&self) -> Result<Option<Scope>, PortError> {
        Ok(self.scope)
    }
    fn receive(&mut self) -> Result<Option<OwnedRx>, PortError> {
        Ok(self.rx.pop_front())
    }
    fn queue(&mut self, s: Scope) -> Result<Self::Queue<'_>, PortError> {
        Ok(Queue(self, s))
    }
    fn close_scope(&mut self, old: Scope) -> Result<(), PortError> {
        self.rx.retain(|rx| rx.scope() != old);
        if self.scope == Some(old) {
            self.scope = None;
        }
        Ok(())
    }
}
#[derive(Debug, Default)]
pub struct Background(pub u32);
impl Work for Background {
    fn step(&mut self, _: u64) -> Result<(), PortError> {
        self.0 += 1;
        Ok(())
    }
}
