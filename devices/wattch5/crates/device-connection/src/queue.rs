//! Real first-fragment queue reservation boundary, shared by history/raw and ordinary event adapters.
/// Result of a real slot reservation; Admitted is only returned after actual ordered FIFO admission.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FirstSlotOutcome<T> {
    /// No real capacity was reserved; the fill callback was never invoked.
    NoSlot,
    /// Fill ran exactly once and its complete bytes entered the actual queue before return.
    Admitted(T),
}
impl<T> core::fmt::Debug for FirstSlotOutcome<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NoSlot => f.write_str("NoSlot"),
            Self::Admitted(_) => f.write_str("Admitted { value: redacted }"),
        }
    }
}
/// Explicit reservation/fill/admission failure, without claiming that uncertain transmission failed cleanly.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FirstSlotError<E> {
    /// Fill rejected the frame; no bytes may have been admitted. Preserve its exact typed cause.
    Fill(E),
    /// Actual transport failed or admission outcome is uncertain; terminate the old connection.
    Transport,
    /// The reservation/serialized actor guarantee could not be upheld; terminate old connection work.
    Contract,
}
impl<E> core::fmt::Debug for FirstSlotError<E> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::Fill(_) => "Fill { cause: redacted }",
            Self::Transport => "Transport",
            Self::Contract => "Contract",
        })
    }
}
/// Actual ordered queue adapter, implemented by the board/GATT transport rather than a readiness boolean.
/// Scope revocation, callback execution and queue publication must share one serialized actor boundary.
/// The callback is at most once by type; no slot means it MUST be dropped without execution.
/// A reservation guarantees capacity for the callback's frame before any producer reserves a DATA ID.
/// A failed/uncertain commit after fill is fatal: callers must close/mute the shared DATA owner and sources.
pub trait FirstFragmentQueue {
    /// Reserve an actual slot, fill it once, and admit the frame before returning Admitted(value).
    /// Buffer contents/length must not be logged. Fill must return a nonzero length within its buffer.
    /// Empty real capacity returns `NoSlot` without calling fill; an `is_ready` precheck is insufficient.
    /// # Errors
    /// Fill errors cancel the reservation without admission; transport/contract uncertainty cannot be retried
    /// as ordinary FIFO-full. Callers decide whether a typed fill rejection is benign or fatal.
    fn with_reserved_slot<T, E>(
        &mut self,
        fill: impl FnOnce(&mut [u8]) -> Result<(usize, T), E>,
    ) -> Result<FirstSlotOutcome<T>, FirstSlotError<E>>;
}
