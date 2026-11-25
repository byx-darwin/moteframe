use device_runtime::PortError;
use moteframe_input::EventQueue;
/// Physical readiness interactions; none grant product authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    /// Source swipe down.
    Down,
    /// Source swipe up.
    Up,
    /// Source swipe left.
    Left,
    /// Source swipe right.
    Right,
    /// Validated source coordinates, without an invented transform.
    Tap {
        /// Horizontal pixel.
        x: u8,
        /// Vertical pixel.
        y: u8,
    },
    /// Touch hold.
    LongPress,
    /// Debounced active-low key release.
    KeyRelease,
    /// One key hold after three seconds.
    KeyLongPress,
}
/// Bounded queue and fallible normalization independent of board/UI interfaces.
#[derive(Debug)]
pub struct InputState {
    queue: EventQueue<Event, 4>,
    last: u8,
    stamp: u64,
    retry: u64,
    probe: bool,
    next: u64,
    candidate: bool,
    stable: bool,
    changed: u64,
    pressed: u64,
    held: bool,
}
impl Default for InputState {
    fn default() -> Self {
        Self::new()
    }
}
impl InputState {
    /// Released key and first touch probe due without an IRQ.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            queue: EventQueue::new(),
            last: 0,
            stamp: 0,
            retry: 0,
            probe: true,
            next: 0,
            candidate: true,
            stable: true,
            changed: 0,
            pressed: 0,
            held: false,
        }
    }
    fn push(&mut self, e: Event) -> Result<(), PortError> {
        self.queue.push(e).map_err(|_| PortError::Unavailable)
    }
    /// Remove one retained event, preserving simultaneous input order.
    pub fn pop(&mut self) -> Option<Event> {
        self.queue.pop()
    }
    /// Actual inline capacity provided by the public MoteFrame queue.
    #[must_use]
    pub const fn queue_capacity(&self) -> usize {
        self.queue.capacity()
    }
    /// One attempt per ten milliseconds; failed reads schedule a half-second probe.
    #[must_use]
    pub fn due(&self, irq_active: bool, now: u64) -> bool {
        now >= self.next && now >= self.retry && (self.probe || irq_active)
    }
    /// Normalize a whole physical register read; errors retain independently queued input.
    /// # Errors
    /// Propagates bus failures, invalid coordinates/gesture and explicit queue overflow.
    pub fn touch(&mut self, frame: Result<[u8; 4], PortError>, now: u64) -> Result<(), PortError> {
        self.next = now.saturating_add(10_000);
        let bytes = match frame {
            Ok(v) => v,
            Err(e) => {
                self.probe = true;
                self.retry = now.saturating_add(500_000);
                return Err(e);
            }
        };
        self.probe = false;
        self.retry = 0;
        let [gesture, x, y, _] = bytes;
        if gesture == 0 {
            self.last = 0;
            return Ok(());
        }
        if gesture == 5 && (x >= 128 || y >= 128) {
            return Err(PortError::InvalidInput);
        }
        let event = match gesture {
            1 => Event::Down,
            2 => Event::Up,
            3 => Event::Left,
            4 => Event::Right,
            5 => Event::Tap { x, y },
            12 => Event::LongPress,
            _ => return Err(PortError::InvalidInput),
        };
        if gesture == self.last && now.saturating_sub(self.stamp) < 200_000 {
            return Ok(());
        }
        if let Err(error) = self.push(event) {
            self.probe = true;
            return Err(error);
        }
        self.last = gesture;
        self.stamp = now;
        Ok(())
    }
    /// Sample real HIGH=released key with30ms debounce and one3s hold.
    /// # Errors
    /// Queue exhaustion is explicit; existing events are preserved.
    pub fn key(&mut self, high: bool, now: u64) -> Result<(), PortError> {
        let (mut candidate, mut stable, mut changed, mut pressed, mut held) = (
            self.candidate,
            self.stable,
            self.changed,
            self.pressed,
            self.held,
        );
        let mut event = None;
        if high != candidate {
            candidate = high;
            changed = now;
        }
        if high != stable && now.saturating_sub(changed) >= 30_000 {
            stable = high;
            if high {
                if !held {
                    event = Some(Event::KeyRelease);
                }
            } else {
                pressed = now;
                held = false;
            }
        }
        if !stable && !held && now.saturating_sub(pressed) >= 3_000_000 {
            held = true;
            event = Some(Event::KeyLongPress);
        }
        if let Some(event) = event {
            self.push(event)?;
        }
        (
            self.candidate,
            self.stable,
            self.changed,
            self.pressed,
            self.held,
        ) = (candidate, stable, changed, pressed, held);
        Ok(())
    }
}
