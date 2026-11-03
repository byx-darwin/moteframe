//! Real service owner behavior against bounded synthetic link ports.
#![allow(clippy::unwrap_used)] // Unexpected fixture failures fail the test.
use moteframe_device_foundation::{
    EnqueueError, Resolution, Sample, Services, TxState,
    model::{LinkModel, Target},
    services::ServiceReport,
};
use moteframe_input::{ButtonConfig, TouchConfig};

fn services() -> Services<LinkModel> {
    services_in_generation(core::num::NonZeroU64::MIN)
}
fn services_in_generation(generation: core::num::NonZeroU64) -> Services<LinkModel> {
    Services::new(
        generation,
        LinkModel::new(),
        ButtonConfig::new(0, Some(100)).unwrap(),
        TouchConfig::new(32, 16, 1, 4, 100).unwrap(),
    )
}
fn round(s: &mut Services<LinkModel>) -> ServiceReport<moteframe_device_foundation::model::Error> {
    let mut r = ServiceReport::default();
    s.round(&mut r);
    r
}

#[test]
fn local_and_backend_full_preserve_original_value_until_admission() {
    let mut s = services();
    round(&mut s);
    s.link_mut().blocks = 2;
    let token = s.enqueue_notification(&[11, 22]).unwrap();
    assert_eq!(s.enqueue_notification(&[99]), Err(EnqueueError::Full));
    assert_eq!(round(&mut s).accepted, 0);
    assert_eq!(round(&mut s).accepted, 0);
    assert_eq!(s.pending_token(), Some(token));
    assert_eq!(round(&mut s).accepted, 1);
    assert_eq!(s.link().last_value(), &[11, 22]);
    assert_eq!(s.link().accepted, 1);
    assert_eq!(s.pending_token(), None);
}

#[test]
fn uncertain_acceptance_never_resubmits_and_requires_the_exact_token() {
    let mut s = services();
    round(&mut s);
    let old = s.enqueue_notification(&[1]).unwrap();
    round(&mut s);
    s.link_mut().uncertain = Some(true);
    let token = s.enqueue_notification(&[2]).unwrap();
    round(&mut s);
    assert_eq!(s.tx_state(), TxState::Uncertain(token));
    assert_eq!(
        s.resolve(old, Resolution::Accepted),
        Err(EnqueueError::WrongToken)
    );
    for _ in 0..8 {
        round(&mut s);
    }
    assert_eq!(s.link().accepted, 2);
    assert_eq!(s.link().submissions, 2);
    s.resolve(token, Resolution::Accepted).unwrap();
    round(&mut s);
    assert_eq!(s.link().submissions, 2);
}

#[test]
fn uncertain_rejection_can_be_explicitly_resolved_then_retried_once() {
    let mut s = services();
    round(&mut s);
    s.link_mut().uncertain = Some(false);
    let token = s.enqueue_notification(&[7]).unwrap();
    round(&mut s);
    round(&mut s);
    assert_eq!(s.link().accepted, 0);
    s.resolve(token, Resolution::NotAccepted).unwrap();
    round(&mut s);
    round(&mut s);
    assert_eq!(s.link().accepted, 1);
    assert_eq!(s.link().submissions, 2);
}

#[test]
fn reconnect_retires_old_pending_and_rejects_old_rx_identity() {
    let mut s = services();
    round(&mut s);
    let target = s.link().target;
    s.link_mut().blocks = 1;
    s.enqueue_notification(&[17]).unwrap();
    round(&mut s);
    s.link_mut().target = Target {
        generation: 2,
        ..target
    };
    s.link_mut().inject(target, &[8]).unwrap();
    let report = round(&mut s);
    assert_eq!(report.retired, 1);
    assert_eq!(report.stale_rx, 1);
    assert_eq!(s.link().accepted, 0);
    assert_eq!(s.pending_token(), None);
    assert!(!s.snapshot().rx_active);
    s.enqueue_notification(&[18]).unwrap();
    round(&mut s);
    assert_eq!(s.link().last_value(), &[18]);
}

#[test]
fn input_pressure_retains_samples_and_later_processes_each_press_once() {
    let mut s = services();
    let mut next = 0u64;
    let total = 40u64;
    let mut full = 0;
    for _ in 0..200 {
        for _ in 0..4 {
            if next == total {
                break;
            }
            match s.enqueue_input(Sample::Button {
                pressed: next.is_multiple_of(2),
                at_us: next,
            }) {
                Ok(()) => next += 1,
                Err(EnqueueError::Full) => {
                    full += 1;
                    break;
                }
                Err(e) => panic!("unexpected {e:?}"),
            }
        }
        round(&mut s);
        if next == total && s.queued_inputs() == 0 {
            break;
        }
    }
    assert!(full > 0);
    assert_eq!(next, total);
    assert_eq!(s.processed_samples(), 40);
    assert_eq!(s.processed_events(), 40);
    assert_eq!(s.snapshot().selection, 0); // twenty presses modulo four
}

#[test]
fn poll_failure_preserves_first_error_and_does_not_stop_input() {
    let mut s = services();
    s.link_mut().poll_faults = 2;
    s.enqueue_input(Sample::Button {
        pressed: true,
        at_us: 0,
    })
    .unwrap();
    let mut report = ServiceReport::default();
    s.round(&mut report);
    s.round(&mut report);
    assert!(report.poll_error.is_some());
    assert_eq!(report.samples, 1);
    assert_eq!(s.snapshot().selection, 1);
    assert_eq!(s.link().polls, 2);
    assert_eq!(s.link().submissions, 0);
}

#[test]
fn empty_committed_rx_is_an_event_and_invalid_input_does_not_poison_the_lane() {
    let mut s = services();
    round(&mut s);
    let target = s.link().target;
    s.link_mut().inject(target, &[]).unwrap();
    assert_eq!(round(&mut s).received, 1);
    assert!(s.snapshot().rx_active);
    s.enqueue_input(Sample::Touch {
        sample: moteframe_input::TouchSample::Up(moteframe_input::Point { x: 1, y: 1 }),
        at_us: 0,
    })
    .unwrap();
    assert!(round(&mut s).input_error.is_some());
    s.enqueue_input(Sample::Button {
        pressed: true,
        at_us: 0,
    })
    .unwrap();
    round(&mut s);
    round(&mut s);
    assert_eq!(s.snapshot().selection, 1);
}

#[test]
fn capacity_race_retains_value_and_invalid_capacity_requires_explicit_repair() {
    let mut s = services();
    round(&mut s);
    s.link_mut().shrink_on_submit = true;
    let token = s.enqueue_notification(&[19, 20]).unwrap();
    assert!(round(&mut s).notify_error.is_some());
    assert_eq!(s.link().accepted, 0);
    let calls = s.link().submissions;
    round(&mut s);
    assert_eq!(s.link().submissions, calls);
    s.link_mut().capacity = 513;
    round(&mut s);
    assert_eq!(s.tx_state(), TxState::Faulted(token));
    s.link_mut().capacity = 8;
    round(&mut s);
    assert_eq!(s.link().accepted, 0);
    s.retry_after_repair(token).unwrap();
    assert_eq!(round(&mut s).accepted, 1);
    assert_eq!(s.link().last_value(), &[19, 20]);
}
#[test]
fn disable_discards_pending_and_does_not_relabel_on_resubscribe() {
    let mut s = services();
    round(&mut s);
    s.link_mut().blocks = 2;
    let old = s.enqueue_notification(&[33]).unwrap();
    round(&mut s);
    s.link_mut().set_enabled(false).unwrap();
    assert_eq!(round(&mut s).retired, 1);
    assert_eq!(
        s.resolve(old, Resolution::Accepted),
        Err(EnqueueError::WrongToken)
    );
    assert_eq!(s.enqueue_notification(&[34]), Err(EnqueueError::NoTarget));
    s.link_mut().set_enabled(true).unwrap();
    round(&mut s);
    assert_eq!(s.link().accepted, 0);
}
#[test]
fn container_debug_redacts_coordinates_values_and_link_details() {
    let mut s = services();
    round(&mut s);
    s.enqueue_notification(&[99, 98]).unwrap();
    let debug = format!("{s:?}");
    assert!(!debug.contains("99"));
    assert!(!debug.contains("LinkModel"));
    let sample = Sample::Button {
        pressed: true,
        at_us: 987_654_321,
    };
    assert_eq!(format!("{sample:?}"), "Sample { .. }");
}

#[test]
fn stale_resolution_cannot_cross_coexisting_or_reconstructed_owners() {
    for resolution in [Resolution::Accepted, Resolution::NotAccepted] {
        let (stale, current) = {
            let mut first = services();
            round(&mut first);
            first.link_mut().uncertain = Some(false);
            let stale = first.enqueue_notification(&[9]).unwrap();
            round(&mut first);
            let mut other = services_in_generation(core::num::NonZeroU64::new(2).unwrap());
            round(&mut other);
            other.link_mut().uncertain = Some(true);
            let current = other.enqueue_notification(&[10]).unwrap();
            round(&mut other);
            assert_eq!(
                other.resolve(stale, resolution),
                Err(EnqueueError::WrongToken)
            );
            assert_eq!(other.tx_state(), TxState::Uncertain(current));
            round(&mut other);
            assert_eq!(other.link().submissions, 1);
            (stale, current)
        };
        let mut reconstructed = services_in_generation(core::num::NonZeroU64::new(3).unwrap());
        round(&mut reconstructed);
        reconstructed.link_mut().uncertain = Some(true);
        let new_token = reconstructed.enqueue_notification(&[11]).unwrap();
        round(&mut reconstructed);
        for retired in [stale, current] {
            assert_eq!(
                reconstructed.resolve(retired, resolution),
                Err(EnqueueError::WrongToken)
            );
        }
        assert_eq!(reconstructed.tx_state(), TxState::Uncertain(new_token));
        round(&mut reconstructed);
        assert_eq!(reconstructed.link().submissions, 1);
    }
}
#[test]
fn subscription_retirement_purges_backend_rx_before_resubscribe() {
    let mut s = services();
    round(&mut s);
    let old = s.link().target;
    s.link_mut().inject(old, &[77]).unwrap();
    s.link_mut().set_enabled(false).unwrap();
    round(&mut s);
    s.link_mut().set_enabled(true).unwrap();
    assert_eq!(round(&mut s).received, 0);
    assert!(!s.snapshot().rx_active);
}
#[test]
fn unobserved_subscription_cycle_cannot_reuse_pending_tx_or_rx_route() {
    let mut s = services();
    round(&mut s);
    let old = s.link().target;
    s.link_mut().inject(old, &[77]).unwrap();
    s.enqueue_notification(&[8]).unwrap();
    s.link_mut().set_enabled(false).unwrap();
    s.link_mut().set_enabled(true).unwrap();
    let report = round(&mut s);
    assert_eq!(report.received, 0);
    assert_eq!(report.retired, 1);
    assert_eq!(s.link().accepted, 0);
}

#[test]
fn subscription_generation_exhaustion_never_wraps_or_reenables() {
    let mut s = services();
    s.link_mut().target.generation = u32::MAX;
    s.link_mut().set_enabled(false).unwrap();
    assert_eq!(
        s.link_mut().set_enabled(true),
        Err(moteframe_device_foundation::model::Error::GenerationExhausted)
    );
    assert!(!s.link().enabled());
    round(&mut s);
    assert!(!s.snapshot().connected);
}
