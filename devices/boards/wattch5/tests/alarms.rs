//! Timer wake ownership remains outside hardware critical sections.
#![allow(clippy::unwrap_used, reason = "Unexpected test fixture failure")]
use board_syd8810::clock::PollAlarms;
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    task::{Wake, Waker},
};
struct Count(AtomicUsize);
impl Wake for Count {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}
#[test]
fn test_should_keep_earliest_shared_wake_and_return_owned_due_wakers() {
    let count = Arc::new(Count(AtomicUsize::new(0)));
    let w = Waker::from(count.clone());
    let mut q = PollAlarms::<1>::new();
    q.schedule(100, &w).unwrap();
    q.schedule(200, &w).unwrap();
    q.schedule(50, &w).unwrap();
    assert!(q.expire(49)[0].is_none());
    let due = q.expire(50);
    assert_eq!(count.0.load(Ordering::SeqCst), 0);
    due.into_iter().flatten().for_each(Waker::wake);
    assert_eq!(count.0.load(Ordering::SeqCst), 1);
    assert!(q.expire(200)[0].is_none());
}
#[test]
fn test_should_report_full_without_replacing_an_existing_deadline() {
    let a = Waker::from(Arc::new(Count(AtomicUsize::new(0))));
    let b = Waker::from(Arc::new(Count(AtomicUsize::new(0))));
    let mut q = PollAlarms::<1>::new();
    q.schedule(10, &a).unwrap();
    assert!(q.schedule(5, &b).is_err());
    assert!(q.expire(5)[0].is_none());
    assert!(q.expire(10)[0].is_some());
}

struct Reenter {
    alarms: Arc<std::sync::Mutex<PollAlarms<1>>>,
    calls: AtomicUsize,
}
impl Wake for Reenter {
    fn wake(self: Arc<Self>) {
        // A callback invoked inside the expiry lock cannot reenter this storage.
        let alarms = self.alarms.try_lock();
        assert!(alarms.is_ok(), "wake ran while alarm storage was locked");
        self.calls.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn test_should_defer_expired_wake_until_storage_can_be_reentered() {
    let alarms = Arc::new(std::sync::Mutex::new(PollAlarms::<1>::new()));
    let task = Arc::new(Reenter {
        alarms: alarms.clone(),
        calls: AtomicUsize::new(0),
    });
    let waker = Waker::from(task.clone());
    alarms.lock().unwrap().schedule(262_144, &waker).unwrap();
    let due = {
        let mut storage = alarms.lock().unwrap();
        assert!(storage.expire(262_143)[0].is_none());
        let due = storage.expire(262_144);
        assert_eq!(task.calls.load(Ordering::SeqCst), 0);
        due
    };
    for wake in due.into_iter().flatten() {
        wake.wake();
    }
    assert_eq!(task.calls.load(Ordering::SeqCst), 1);
    assert!(alarms.lock().unwrap().expire(u64::MAX)[0].is_none());
}
