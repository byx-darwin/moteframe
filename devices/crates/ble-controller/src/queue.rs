//! Owned fixed-capacity storage; synchronization belongs to the runner.

/// The queue rejected admission and retained all older elements.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QueueFull;

/// A bounded FIFO with no allocator, overwrite or internal synchronization.
#[derive(Debug)]
pub struct OwnedQueue<T: Copy, const N: usize> {
    elements: [Option<T>; N],
    head: usize,
    len: usize,
}
impl<T: Copy, const N: usize> Default for OwnedQueue<T, N> {
    fn default() -> Self {
        Self::new()
    }
}
impl<T: Copy, const N: usize> OwnedQueue<T, N> {
    /// Empty storage; a zero-capacity queue always rejects admission.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            elements: [None; N],
            head: 0,
            len: 0,
        }
    }
    /// Number of owned elements.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.len
    }
    /// Whether storage is empty.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }
    /// Whether admission would fail.
    #[must_use]
    pub const fn is_full(&self) -> bool {
        self.len == N
    }
    /// Retains an owned element at the tail.
    /// # Errors
    /// Returns `QueueFull` without modifying existing elements.
    pub fn push(&mut self, element: T) -> Result<(), QueueFull> {
        if self.is_full() {
            return Err(QueueFull);
        }
        let distance = N - self.head;
        let tail = if self.len >= distance {
            self.len - distance
        } else {
            self.head + self.len
        };
        self.elements[tail] = Some(element);
        self.len += 1;
        Ok(())
    }
    /// Observes the oldest owned element without removing it.
    #[must_use]
    pub fn front(&self) -> Option<&T> {
        if self.is_empty() {
            None
        } else {
            self.elements[self.head].as_ref()
        }
    }
    /// Removes exactly one oldest element.
    pub fn pop(&mut self) -> Option<T> {
        if self.is_empty() {
            return None;
        }
        let element = self.elements[self.head].take();
        self.head = if self.head == N - 1 { 0 } else { self.head + 1 };
        self.len -= 1;
        element
    }
    /// Drops all queued work, for an explicit reset or disconnected epoch.
    pub fn clear(&mut self) {
        self.elements = [None; N];
        self.head = 0;
        self.len = 0;
    }
}
