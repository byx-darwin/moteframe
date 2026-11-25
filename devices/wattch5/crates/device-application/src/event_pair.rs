//! Bounded composition of independently owned producers for the single physical connection pump.
use crate::{EventOffer, EventRoute, EventSource, SourceFailure};
use core::cell::Cell;
#[derive(Clone, Copy, PartialEq, Eq)]
enum ChildToken<A, B> {
    First(A),
    Second(B),
}
/// Opaque exact child/offer token. Callers cannot construct or replace its producer identity.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct EventPairToken<A, B> {
    child: ChildToken<A, B>,
    generation: u64,
}
impl<A, B> core::fmt::Debug for EventPairToken<A, B> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("EventPairToken").finish_non_exhaustive()
    }
}
/// Own two sources, rotating after real whole-event admission or a benign unready preparation.
/// Nest pairs to assemble any statically selected product without heap storage or dynamic dispatch.
/// Source handles can reference the same checked service actors used for requests; no extra
/// producer, DATA lane, authority or payload copy is introduced here.
pub struct EventPair<A: EventSource, B: EventSource> {
    first: A,
    second: B,
    preferred_second: bool,
    selected_second: Cell<Option<bool>>,
    pending: Option<EventPairToken<A::Token, B::Token>>,
    generation: u64,
    closed: bool,
    cleaned: [bool; 2],
}
impl<A: EventSource, B: EventSource> core::fmt::Debug for EventPair<A, B> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("EventPair")
            .field("pending", &self.pending.is_some())
            .field("closed", &self.closed)
            .finish_non_exhaustive()
    }
}
impl<A: EventSource, B: EventSource> EventPair<A, B> {
    /// Permanently bind both producers for this physical application connection.
    pub const fn new(first: A, second: B) -> Self {
        Self {
            first,
            second,
            preferred_second: false,
            selected_second: Cell::new(None),
            pending: None,
            generation: 0,
            closed: false,
            cleaned: [false; 2],
        }
    }
    fn child_route(&self, second: bool) -> Result<Option<EventRoute>, SourceFailure> {
        if second {
            self.second.route()
        } else {
            self.first.route()
        }
    }
}
impl<A: EventSource, B: EventSource> EventSource for EventPair<A, B> {
    type Token = EventPairToken<A::Token, B::Token>;
    fn route(&self) -> Result<Option<EventRoute>, SourceFailure> {
        if self.must_end_session() {
            return Err(SourceFailure::Denied);
        }
        if self.pending.is_some() {
            return Err(SourceFailure::Invalid);
        }
        self.selected_second.set(None);
        for second in [self.preferred_second, !self.preferred_second] {
            match self.child_route(second) {
                Ok(Some(route)) => {
                    self.selected_second.set(Some(second));
                    return Ok(Some(route));
                }
                Ok(None) | Err(SourceFailure::NotReady) => {}
                Err(error) => return Err(error),
            }
        }
        Ok(None)
    }
    fn prepare(&mut self, out: &mut [u8]) -> Result<EventOffer<Self::Token>, SourceFailure> {
        if self.must_end_session() {
            return Err(SourceFailure::Denied);
        }
        if self.pending.is_some() {
            return Err(SourceFailure::Invalid);
        }
        let generation = self
            .generation
            .checked_add(1)
            .ok_or(SourceFailure::Capacity)?;
        let second = self.selected_second.take().ok_or(SourceFailure::NotReady)?;
        let prepared = if second {
            self.second.prepare(out).map(|offer| {
                (
                    ChildToken::Second(offer.token),
                    offer.length,
                    offer.data_message,
                )
            })
        } else {
            self.first.prepare(out).map(|offer| {
                (
                    ChildToken::First(offer.token),
                    offer.length,
                    offer.data_message,
                )
            })
        };
        let (child, length, data_message) = match prepared {
            Err(SourceFailure::NotReady) => {
                // No message began and no token/credit was consumed. Probe the other source on
                // the next route call; never change the route within this FIFO reservation.
                self.preferred_second = !second;
                return Err(SourceFailure::NotReady);
            }
            result => result?,
        };
        let token = EventPairToken { child, generation };
        self.generation = generation;
        self.pending = Some(token);
        Ok(EventOffer {
            token,
            length,
            data_message,
        })
    }
    fn authorized(&self, token: Self::Token) -> bool {
        !self.must_end_session()
            && self.pending == Some(token)
            && match token.child {
                ChildToken::First(token) => self.first.authorized(token),
                ChildToken::Second(token) => self.second.authorized(token),
            }
    }
    fn must_end_session(&self) -> bool {
        self.closed || self.first.must_end_session() || self.second.must_end_session()
    }
    fn admitted(&mut self, token: Self::Token) -> Result<(), SourceFailure> {
        if !self.authorized(token) {
            return Err(SourceFailure::Denied);
        }
        match token.child {
            ChildToken::First(token) => {
                self.first.admitted(token)?;
                self.preferred_second = true;
            }
            ChildToken::Second(token) => {
                self.second.admitted(token)?;
                self.preferred_second = false;
            }
        }
        self.pending = None;
        Ok(())
    }
    fn connection_closed(&mut self) -> Result<(), SourceFailure> {
        self.closed = true;
        self.pending = None;
        self.selected_second.set(None);
        let first = if self.cleaned[0] {
            Ok(())
        } else {
            self.first.connection_closed()
        };
        let second = if self.cleaned[1] {
            Ok(())
        } else {
            self.second.connection_closed()
        };
        self.cleaned[0] = first.is_ok();
        self.cleaned[1] = second.is_ok();
        first.and(second)
    }
}
