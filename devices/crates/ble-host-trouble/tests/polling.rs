//! Cooperative split-runner scheduling matches the upstream select order.
use ble_host_trouble::poll_host_runners;
use core::{
    future::Future,
    pin::{Pin, pin},
    task::{Context, Poll, Waker},
};
use std::{cell::RefCell, rc::Rc};
struct Probe {
    lane: u8,
    calls: Rc<RefCell<Vec<u8>>>,
    result: Poll<Result<u8, u8>>,
}
impl Future for Probe {
    type Output = Result<u8, u8>;
    fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Self::Output> {
        self.calls.borrow_mut().push(self.lane);
        self.result
    }
}
fn check(results: [Poll<Result<u8, u8>>; 3], expected: Poll<Result<u8, u8>>, calls: &[u8]) {
    let trace = Rc::new(RefCell::new(Vec::new()));
    let mut tx = pin!(Probe {
        lane: 0,
        calls: trace.clone(),
        result: results[0]
    });
    let mut rx = pin!(Probe {
        lane: 1,
        calls: trace.clone(),
        result: results[1]
    });
    let mut control = pin!(Probe {
        lane: 2,
        calls: trace.clone(),
        result: results[2]
    });
    let mut cx = Context::from_waker(Waker::noop());
    assert_eq!(
        poll_host_runners(tx.as_mut(), rx.as_mut(), control.as_mut(), &mut cx),
        expected
    );
    assert_eq!(&*trace.borrow(), calls);
}
#[test]
fn test_should_poll_each_pending_lane_in_upstream_order() {
    check([Poll::Pending; 3], Poll::Pending, &[0, 1, 2]);
}
#[test]
fn test_should_stop_on_first_ready_lane_and_preserve_its_result() {
    check(
        [Poll::Ready(Ok(9)), Poll::Ready(Err(7)), Poll::Ready(Err(8))],
        Poll::Ready(Ok(9)),
        &[0],
    );
    check(
        [Poll::Pending, Poll::Ready(Err(7)), Poll::Ready(Err(8))],
        Poll::Ready(Err(7)),
        &[0, 1],
    );
    check(
        [Poll::Pending, Poll::Pending, Poll::Ready(Err(8))],
        Poll::Ready(Err(8)),
        &[0, 1, 2],
    );
}

#[derive(Debug)]
struct DropProbe {
    lane: u8,
    dropped: Rc<core::cell::Cell<u8>>,
    ready: bool,
}
impl Future for DropProbe {
    type Output = Result<(), u8>;
    fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Self::Output> {
        if self.ready {
            Poll::Ready(Err(self.lane))
        } else {
            Poll::Pending
        }
    }
}
impl Drop for DropProbe {
    fn drop(&mut self) {
        self.dropped.set(self.dropped.get() | (1 << self.lane));
    }
}
#[test]
fn test_should_drop_all_owned_host_lanes_before_reporting_first_completion() {
    let dropped = Rc::new(core::cell::Cell::new(0));
    let make = |lane| DropProbe {
        lane,
        dropped: dropped.clone(),
        ready: lane == 0,
    };
    let mut tx = pin!(Some(make(0)));
    let mut rx = pin!(Some(make(1)));
    let mut control = pin!(Some(make(2)));
    let mut future = pin!(
        ble_host_trouble::HostRunners::new(tx.as_mut(), rx.as_mut(), control.as_mut())
            .unwrap_or_else(|error| panic!("runner slots {error:?}"))
    );
    assert_eq!(
        future
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop())),
        Poll::Ready(Err(0))
    );
    assert_eq!(dropped.get(), 7);
    assert_eq!(
        future
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop())),
        Poll::Pending
    );
}
#[test]
fn test_should_drop_all_owned_host_lanes_when_pending_aggregate_is_cancelled() {
    let dropped = Rc::new(core::cell::Cell::new(0));
    let make = |lane| DropProbe {
        lane,
        dropped: dropped.clone(),
        ready: false,
    };
    let mut tx = pin!(Some(make(0)));
    let mut rx = pin!(Some(make(1)));
    let mut control = pin!(Some(make(2)));
    {
        let mut future = pin!(
            ble_host_trouble::HostRunners::new(tx.as_mut(), rx.as_mut(), control.as_mut())
                .unwrap_or_else(|error| panic!("runner slots {error:?}"))
        );
        assert_eq!(
            future
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop())),
            Poll::Pending
        );
        assert_eq!(dropped.get(), 0);
    }
    assert_eq!(dropped.get(), 7);
    assert!(tx.as_ref().get_ref().is_none());
    assert!(rx.as_ref().get_ref().is_none());
    assert!(control.as_ref().get_ref().is_none());
}
