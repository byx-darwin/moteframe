// Copyright 2026 MoteFrame contributors
// SPDX-License-Identifier: Apache-2.0
// See LICENSE and NOTICE for license and source attribution.

//! Notification and committed-write behavior, boundary, and ownership regressions.

use std::{cell::Cell, rc::Rc};

use moteframe_ble::{
    CheckedNotifier, NotificationBackend, NotificationRef, NotifyError, QueueAccepted,
    WriteBufferError, WriteMeta, WriteSlot,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Target {
    generation: u32,
    characteristic: u16,
    bearer: u8,
}

const TARGET: Target = Target {
    generation: 3,
    characteristic: 17,
    bearer: 1,
};

#[derive(Debug, PartialEq, Eq)]
struct Failure(&'static str);

#[derive(Debug)]
struct Backend {
    target: Target,
    capacity: Rc<Cell<usize>>,
    ownership: Box<u8>,
    queries: Cell<usize>,
    submissions: usize,
    query_failure: Option<&'static str>,
    submit_failure: Option<&'static str>,
    race_capacity: Option<usize>,
    race_target: Option<Target>,
    submitted: Vec<(Target, Vec<u8>)>,
}

impl Backend {
    fn new(capacity: usize) -> Self {
        Self {
            target: TARGET,
            capacity: Rc::new(Cell::new(capacity)),
            ownership: Box::new(42),
            queries: Cell::new(0),
            submissions: 0,
            query_failure: None,
            submit_failure: None,
            race_capacity: None,
            race_target: None,
            submitted: Vec::new(),
        }
    }
}

impl NotificationBackend for Backend {
    type Target = Target;
    type Error = Failure;

    fn notification_capacity(&self, target: Target) -> Result<usize, Failure> {
        self.queries.set(self.queries.get() + 1);
        if let Some(reason) = self.query_failure {
            return Err(Failure(reason));
        }
        if target != self.target {
            return Err(Failure("stale identity"));
        }
        Ok(self.capacity.get())
    }

    fn try_queue_notification(&mut self, target: Target, value: &[u8]) -> Result<(), Failure> {
        self.submissions += 1;
        if let Some(capacity) = self.race_capacity {
            self.capacity.set(capacity);
        }
        if let Some(current) = self.race_target {
            self.target = current;
        }
        if target != self.target {
            return Err(Failure("stale identity"));
        }
        if value.len() > self.capacity.get() {
            return Err(Failure("capacity changed"));
        }
        if let Some(reason) = self.submit_failure {
            return Err(Failure(reason));
        }
        self.submitted.push((target, value.to_vec()));
        Ok(())
    }
}

#[test]
fn notifier_construction_and_recovery_do_not_touch_backend() {
    let backend = Backend::new(23);
    let identity = std::ptr::from_ref(&*backend.ownership);
    let notifier = CheckedNotifier::new(backend);
    let backend = notifier.into_inner();
    assert_eq!(backend.queries.get(), 0);
    assert_eq!(backend.submissions, 0);
    assert_eq!(backend.target, TARGET);
    assert!(std::ptr::eq(
        std::ptr::from_ref(&*backend.ownership),
        identity
    ));
}

#[test]
fn accepted_means_a_single_copied_queue_input() {
    let mut backend = Backend::new(23);
    {
        let mut notifier = CheckedNotifier::new(NotificationRef::new(&mut backend));
        let mut value = [1, 2, 3];
        let accepted: Result<QueueAccepted, NotifyError<Failure>> =
            notifier.try_notify(TARGET, &value);
        assert_eq!(accepted, Ok(QueueAccepted));
        value.fill(99);
    }
    assert_eq!(backend.queries.get(), 1);
    assert_eq!(backend.submissions, 1);
    assert_eq!(backend.submitted, vec![(TARGET, vec![1, 2, 3])]);
}

#[test]
fn borrowed_dyn_backend_preserves_original_identity_without_calls() {
    let mut backend = Backend::new(8);
    let original = std::ptr::from_mut(&mut backend);
    let erased: &mut dyn NotificationBackend<Target = Target, Error = Failure> = &mut backend;
    let original_erased = std::ptr::from_mut(erased);
    let notifier = CheckedNotifier::new(NotificationRef::new(erased));
    let recovered = notifier.into_inner().into_inner();
    assert!(std::ptr::eq(std::ptr::from_mut(recovered), original_erased));
    assert_eq!(recovered.try_queue_notification(TARGET, &[4]), Ok(()));
    assert!(std::ptr::eq(std::ptr::from_ref(&backend), original));
    assert_eq!(backend.queries.get(), 0);
    assert_eq!(backend.submitted, vec![(TARGET, vec![4])]);
}

#[test]
fn borrowed_dyn_backend_forwards_query_and_submission() {
    let mut backend = Backend::new(8);
    let erased: &mut dyn NotificationBackend<Target = Target, Error = Failure> = &mut backend;
    let mut notifier = CheckedNotifier::new(NotificationRef::new(erased));
    assert_eq!(notifier.try_notify(TARGET, &[8]), Ok(QueueAccepted));
    assert_eq!(backend.queries.get(), 1);
    assert_eq!(backend.submitted, vec![(TARGET, vec![8])]);
}

#[test]
fn capacity_is_queried_fresh_for_each_call() {
    let backend = Backend::new(3);
    let capacity = Rc::clone(&backend.capacity);
    let mut notifier = CheckedNotifier::new(backend);
    assert_eq!(notifier.try_notify(TARGET, &[1, 2, 3]), Ok(QueueAccepted));
    capacity.set(2);
    assert_eq!(
        notifier.try_notify(TARGET, &[1, 2, 3]),
        Err(NotifyError::TooLong {
            length: 3,
            capacity: 2
        })
    );
    assert_eq!(notifier.try_notify(TARGET, &[1, 2]), Ok(QueueAccepted));
    let backend = notifier.into_inner();
    assert_eq!(backend.queries.get(), 3);
    assert_eq!(backend.submissions, 2);
}

#[test]
fn every_target_identity_component_is_checked_by_backend() {
    let mut notifier = CheckedNotifier::new(Backend::new(8));
    for stale in [
        Target {
            generation: 2,
            ..TARGET
        },
        Target {
            characteristic: 18,
            ..TARGET
        },
        Target {
            bearer: 2,
            ..TARGET
        },
    ] {
        assert_eq!(
            notifier.try_notify(stale, &[1]),
            Err(NotifyError::Backend(Failure("stale identity")))
        );
    }
    let backend = notifier.into_inner();
    assert_eq!(backend.queries.get(), 3);
    assert_eq!(backend.submissions, 0);
}

#[test]
fn raw_query_failure_prevents_submission_without_retry() {
    let mut backend = Backend::new(8);
    backend.query_failure = Some("CCCD unavailable");
    let mut notifier = CheckedNotifier::new(backend);
    assert_eq!(
        notifier.try_notify(TARGET, &[1]),
        Err(NotifyError::Backend(Failure("CCCD unavailable")))
    );
    let backend = notifier.into_inner();
    assert_eq!(backend.queries.get(), 1);
    assert_eq!(backend.submissions, 0);
}

#[test]
fn raw_submission_failure_is_returned_without_retry() {
    let mut backend = Backend::new(8);
    backend.submit_failure = Some("queue full");
    let mut notifier = CheckedNotifier::new(backend);
    assert_eq!(
        notifier.try_notify(TARGET, &[1]),
        Err(NotifyError::Backend(Failure("queue full")))
    );
    let backend = notifier.into_inner();
    assert_eq!(backend.queries.get(), 1);
    assert_eq!(backend.submissions, 1);
    assert!(backend.submitted.is_empty());
}

#[test]
fn backend_revalidates_capacity_race_at_submission() {
    let mut backend = Backend::new(8);
    backend.race_capacity = Some(1);
    let mut notifier = CheckedNotifier::new(backend);
    assert_eq!(
        notifier.try_notify(TARGET, &[1, 2]),
        Err(NotifyError::Backend(Failure("capacity changed")))
    );
    let backend = notifier.into_inner();
    assert_eq!(backend.queries.get(), 1);
    assert_eq!(backend.submissions, 1);
    assert!(backend.submitted.is_empty());
}

#[test]
fn backend_revalidates_connection_race_at_submission() {
    let mut backend = Backend::new(8);
    backend.race_target = Some(Target {
        generation: 4,
        ..TARGET
    });
    let mut notifier = CheckedNotifier::new(backend);
    assert_eq!(
        notifier.try_notify(TARGET, &[1]),
        Err(NotifyError::Backend(Failure("stale identity")))
    );
    let backend = notifier.into_inner();
    assert_eq!(backend.submissions, 1);
    assert!(backend.submitted.is_empty());
}

#[test]
fn oversized_backend_capacity_is_rejected_even_for_empty_value() {
    let mut notifier = CheckedNotifier::new(Backend::new(513));
    assert_eq!(
        notifier.try_notify(TARGET, &[]),
        Err(NotifyError::InvalidCapacity { capacity: 513 })
    );
    assert_eq!(notifier.into_inner().submissions, 0);
}

#[test]
fn notification_accepts_512_but_never_truncates_513() {
    let mut notifier = CheckedNotifier::new(Backend::new(512));
    assert_eq!(notifier.try_notify(TARGET, &[5; 512]), Ok(QueueAccepted));
    assert_eq!(
        notifier.try_notify(TARGET, &[5; 513]),
        Err(NotifyError::TooLong {
            length: 513,
            capacity: 512
        })
    );
    let backend = notifier.into_inner();
    assert_eq!(backend.submissions, 1);
    assert_eq!(backend.submitted[0].1, [5; 512]);
}

#[test]
fn zero_capacity_accepts_empty_and_rejects_nonempty() {
    let mut notifier = CheckedNotifier::new(Backend::new(0));
    assert_eq!(notifier.try_notify(TARGET, &[]), Ok(QueueAccepted));
    assert_eq!(
        notifier.try_notify(TARGET, &[1]),
        Err(NotifyError::TooLong {
            length: 1,
            capacity: 0
        })
    );
    assert_eq!(notifier.into_inner().submissions, 1);
}

#[test]
fn slot_peek_and_take_preserve_complete_event_and_output_tail() {
    let mut slot = WriteSlot::<Target, 8>::new();
    assert_eq!(slot.peek(), None);
    assert_eq!(slot.push(TARGET, &[1, 2, 3]), Ok(()));
    let meta = WriteMeta {
        target: TARGET,
        length: 3,
    };
    assert_eq!(slot.peek(), Some((meta, &[1, 2, 3][..])));
    let mut output = [77; 8];
    assert_eq!(slot.take_into(&mut output), Ok(Some(meta)));
    assert_eq!(output, [1, 2, 3, 77, 77, 77, 77, 77]);
    assert_eq!(slot.peek(), None);
    assert_eq!(slot.take_into(&mut output), Ok(None));
    assert_eq!(output, [1, 2, 3, 77, 77, 77, 77, 77]);
}

#[test]
fn full_slot_rejects_first_and_never_relabels_existing_connection() {
    let mut slot = WriteSlot::<Target, 2>::new();
    assert_eq!(slot.push(TARGET, &[1, 2]), Ok(()));
    let next = Target {
        generation: 4,
        ..TARGET
    };
    assert_eq!(slot.push(next, &[9; 3]), Err(WriteBufferError::Full));
    assert_eq!(slot.push(next, &[]), Err(WriteBufferError::Full));
    assert_eq!(
        slot.peek(),
        Some((
            WriteMeta {
                target: TARGET,
                length: 2
            },
            &[1, 2][..]
        ))
    );
}

#[test]
fn oversized_write_rejects_without_creating_or_truncating_event() {
    let mut slot = WriteSlot::<Target, 2>::new();
    assert_eq!(
        slot.push(TARGET, &[1, 2, 3]),
        Err(WriteBufferError::TooLong {
            length: 3,
            capacity: 2
        })
    );
    assert_eq!(slot.peek(), None);
    assert_eq!(slot.push(TARGET, &[1, 2]), Ok(()));
    assert_eq!(
        slot.peek(),
        Some((
            WriteMeta {
                target: TARGET,
                length: 2
            },
            &[1, 2][..]
        ))
    );
}

#[test]
fn too_small_output_leaves_slot_and_destination_untouched() {
    let mut slot = WriteSlot::<Target, 8>::new();
    assert_eq!(slot.push(TARGET, &[1, 2, 3]), Ok(()));
    let mut output = [77; 2];
    assert_eq!(
        slot.take_into(&mut output),
        Err(WriteBufferError::OutputTooSmall {
            length: 3,
            capacity: 2
        })
    );
    assert_eq!(output, [77; 2]);
    assert_eq!(
        slot.peek(),
        Some((
            WriteMeta {
                target: TARGET,
                length: 3
            },
            &[1, 2, 3][..]
        ))
    );
    let mut exact = [0; 3];
    assert_eq!(
        slot.take_into(&mut exact),
        Ok(Some(WriteMeta {
            target: TARGET,
            length: 3
        }))
    );
    assert_eq!(exact, [1, 2, 3]);
}

#[test]
fn clear_explicitly_discards_and_returns_original_metadata() {
    let mut slot = WriteSlot::<Target, 8>::new();
    assert_eq!(slot.clear(), None);
    assert_eq!(slot.push(TARGET, &[1, 2]), Ok(()));
    assert_eq!(
        slot.clear(),
        Some(WriteMeta {
            target: TARGET,
            length: 2
        })
    );
    assert_eq!(slot.peek(), None);
    let next = Target {
        generation: 4,
        ..TARGET
    };
    assert_eq!(slot.push(next, &[8]), Ok(()));
    assert_eq!(
        slot.peek(),
        Some((
            WriteMeta {
                target: next,
                length: 1
            },
            &[8][..]
        ))
    );
}

#[test]
fn zero_storage_keeps_empty_event_distinct_from_no_event() {
    let mut slot = WriteSlot::<Target, 0>::new();
    assert_eq!(slot.take_into(&mut []), Ok(None));
    assert_eq!(
        slot.push(TARGET, &[1]),
        Err(WriteBufferError::TooLong {
            length: 1,
            capacity: 0
        })
    );
    assert_eq!(slot.push(TARGET, &[]), Ok(()));
    let meta = WriteMeta {
        target: TARGET,
        length: 0,
    };
    assert_eq!(slot.peek(), Some((meta, &[][..])));
    assert_eq!(slot.push(TARGET, &[]), Err(WriteBufferError::Full));
    assert_eq!(slot.take_into(&mut []), Ok(Some(meta)));
    assert_eq!(slot.take_into(&mut []), Ok(None));
    assert_eq!(slot.push(TARGET, &[]), Ok(()));
    assert_eq!(slot.clear(), Some(meta));
}

#[test]
fn oversized_allocation_still_limits_values_to_512() {
    let mut slot = WriteSlot::<Target, 1024>::new();
    assert_eq!(
        slot.push(TARGET, &[0; 513]),
        Err(WriteBufferError::TooLong {
            length: 513,
            capacity: 512
        })
    );
    assert_eq!(slot.peek(), None);
    assert_eq!(slot.push(TARGET, &[6; 512]), Ok(()));
    let mut output = [77; 513];
    assert_eq!(
        slot.take_into(&mut output),
        Ok(Some(WriteMeta {
            target: TARGET,
            length: 512
        }))
    );
    assert_eq!(&output[..512], &[6; 512]);
    assert_eq!(output[512], 77);
}
