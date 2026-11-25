//! Stack-separated polling of the upstream independent Host runners.
use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

/// Poll split Host futures in upstream order: transmit, receive, then control.
///
/// Call with the three externally pinned futures from `Runner::split`. All
/// pending lanes are serviced once per call; the first completion is returned
/// unchanged and later lanes are not polled. Stop polling after completion.
/// Each lane has its own scratch frame, without extra buffers or allocation.
pub fn poll_host_runners<T, R, C>(
    tx: Pin<&mut T>,
    rx: Pin<&mut R>,
    control: Pin<&mut C>,
    cx: &mut Context<'_>,
) -> Poll<T::Output>
where
    T: Future,
    R: Future<Output = T::Output>,
    C: Future<Output = T::Output>,
{
    if let Poll::Ready(result) = poll_lane(tx, cx) {
        return Poll::Ready(result);
    }
    if let Poll::Ready(result) = poll_lane(rx, cx) {
        return Poll::Ready(result);
    }
    poll_lane(control, cx)
}
#[inline(never)]
fn poll_lane<F: Future>(future: Pin<&mut F>, cx: &mut Context<'_>) -> Poll<F::Output> {
    future.poll(cx)
}

/// Missing or already-completed split-runner slots cannot start an owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostRunnersClosed;

/// Own the lifecycle of three externally pinned optional Host futures.
///
/// Child futures stay in their original storage. First completion drops every
/// child in place before returning; dropping a pending owner also cancels all
/// children. No child projection or task allocation requires unsafe code.
pub struct HostRunners<'a, T: Future, R: Future<Output = T::Output>, C: Future<Output = T::Output>>
{
    tx: Pin<&'a mut Option<T>>,
    rx: Pin<&'a mut Option<R>>,
    control: Pin<&'a mut Option<C>>,
}
impl<T: Future, R: Future<Output = T::Output>, C: Future<Output = T::Output>> core::fmt::Debug
    for HostRunners<'_, T, R, C>
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("HostRunners")
            .field("closed", &self.tx.as_ref().get_ref().is_none())
            .finish_non_exhaustive()
    }
}
impl<'a, T: Future, R: Future<Output = T::Output>, C: Future<Output = T::Output>>
    HostRunners<'a, T, R, C>
{
    /// Borrow all three pinned slots exclusively until completion/cancellation.
    /// # Errors
    /// Rejects an empty slot without polling any lane.
    pub fn new(
        tx: Pin<&'a mut Option<T>>,
        rx: Pin<&'a mut Option<R>>,
        control: Pin<&'a mut Option<C>>,
    ) -> Result<Self, HostRunnersClosed> {
        if tx.as_ref().get_ref().is_none()
            || rx.as_ref().get_ref().is_none()
            || control.as_ref().get_ref().is_none()
        {
            return Err(HostRunnersClosed);
        }
        Ok(Self { tx, rx, control })
    }
    fn clear(&mut self) {
        self.tx.as_mut().set(None);
        self.rx.as_mut().set(None);
        self.control.as_mut().set(None);
    }
}
impl<T: Future, R: Future<Output = T::Output>, C: Future<Output = T::Output>> Future
    for HostRunners<'_, T, R, C>
{
    type Output = T::Output;
    // Inline only the small owner/cleanup dispatcher; each lane keeps its
    // separate non-inlined scratch boundary.
    #[inline]
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        if this.tx.as_ref().get_ref().is_none() {
            return Poll::Pending;
        }
        // Construction checked all slots; exclusive pinned borrows prevent
        // replacement until this owner clears them. A completed owner is fused.
        let (Some(tx), Some(rx), Some(control)) = (
            this.tx.as_mut().as_pin_mut(),
            this.rx.as_mut().as_pin_mut(),
            this.control.as_mut().as_pin_mut(),
        ) else {
            return Poll::Pending;
        };
        let result = poll_host_runners(tx, rx, control, cx);
        if result.is_ready() {
            this.clear();
        }
        result
    }
}
impl<T: Future, R: Future<Output = T::Output>, C: Future<Output = T::Output>> Drop
    for HostRunners<'_, T, R, C>
{
    fn drop(&mut self) {
        self.clear();
    }
}
