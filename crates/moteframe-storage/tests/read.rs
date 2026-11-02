// Copyright 2026 MoteFrame contributors
// SPDX-License-Identifier: Apache-2.0
// See LICENSE and NOTICE for license and source attribution.

//! Checked reads, provider failures, dynamic bounds, and ownership recovery.

use core::ptr;
use moteframe_core::ReadAt;
use moteframe_storage::{RangeError, ReadError, ReadRef, ReadWindow, SliceSource};
use std::{cell::Cell, rc::Rc};

#[derive(Debug, PartialEq, Eq)]
struct Failure(u16);

#[derive(Default)]
struct State {
    capacity: Cell<u32>,
    capacity_calls: Cell<usize>,
    reads: Cell<usize>,
    offset: Cell<u32>,
    length: Cell<usize>,
    fail: Cell<bool>,
}

// Deliberately neither Copy nor Debug: rejected construction must return it.
struct Source {
    state: Rc<State>,
    identity: Box<u8>,
}

impl ReadAt for Source {
    type Error = Failure;

    fn capacity(&self) -> u32 {
        self.state
            .capacity_calls
            .set(self.state.capacity_calls.get() + 1);
        self.state.capacity.get()
    }

    fn read_exact(&mut self, offset: u32, out: &mut [u8]) -> Result<(), Self::Error> {
        self.state.reads.set(self.state.reads.get() + 1);
        self.state.offset.set(offset);
        self.state.length.set(out.len());
        if self.state.fail.get() {
            if let Some(first) = out.first_mut() {
                *first = 0xe1;
            }
            return Err(Failure(731));
        }
        out.fill(0x35);
        Ok(())
    }
}

fn source(capacity: u32) -> (Source, Rc<State>) {
    let state = Rc::new(State::default());
    state.capacity.set(capacity);
    (
        Source {
            state: state.clone(),
            identity: Box::new(91),
        },
        state,
    )
}

fn window(source: Source, start: u32, length: u32) -> ReadWindow<Source> {
    ReadWindow::new(source, start, length).unwrap_or_else(|e| panic!("{e:?}"))
}

#[test]
fn slice_reads_exact_range_and_returns_identical_slice() {
    let bytes = [10, 20, 30, 40, 50];
    let input = &bytes[1..];
    let mut reader = SliceSource::new(input).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(reader.capacity(), 4);
    let mut output = [0; 2];
    assert_eq!(reader.read_exact(1, &mut output), Ok(()));
    assert_eq!(output, [30, 40]);
    assert!(ptr::eq(reader.into_inner(), input));
}

#[test]
fn slice_rejects_all_invalid_ranges_without_changing_output() {
    let mut reader = SliceSource::new(&[1, 2, 3]).unwrap_or_else(|e| panic!("{e:?}"));
    for offset in [2, 3, 4, u32::MAX] {
        let mut output = [0x77; 2];
        assert_eq!(
            reader.read_exact(offset, &mut output),
            Err(RangeError::OutOfBounds)
        );
        assert_eq!(output, [0x77; 2]);
    }
}

#[test]
fn slice_empty_reads_accept_end_and_reject_past_end() {
    let mut reader = SliceSource::new(&[1, 2, 3]).unwrap_or_else(|e| panic!("{e:?}"));
    for offset in [0, 2, 3] {
        assert_eq!(reader.read_exact(offset, &mut []), Ok(()));
    }
    assert_eq!(reader.read_exact(4, &mut []), Err(RangeError::OutOfBounds));
    let mut empty = SliceSource::new(&[]).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(empty.capacity(), 0);
    assert_eq!(empty.read_exact(0, &mut []), Ok(()));
    assert_eq!(empty.read_exact(1, &mut []), Err(RangeError::OutOfBounds));
}

#[test]
fn read_ref_forwards_dyn_provider_and_returns_original_borrow() {
    let (mut provider, state) = source(20);
    let dynamic: &mut dyn ReadAt<Error = Failure> = &mut provider;
    let identity = ptr::from_ref(&*dynamic);
    let mut borrowed = ReadRef::new(dynamic);
    assert_eq!(borrowed.capacity(), 20);
    let mut output = [0; 2];
    assert_eq!(borrowed.read_exact(7, &mut output), Ok(()));
    assert_eq!(output, [0x35; 2]);
    assert_eq!(state.offset.get(), 7);
    assert_eq!(state.reads.get(), 1);
    assert!(ptr::eq(ptr::from_ref(&*borrowed.into_inner()), identity));
    assert_eq!(state.reads.get(), 1);
    assert_eq!(state.capacity_calls.get(), 1);
}

#[test]
fn read_ref_preserves_provider_failure_without_retry() {
    let (mut provider, state) = source(20);
    state.fail.set(true);
    let mut borrowed = ReadRef::new(&mut provider);
    let mut output = [0x88; 2];
    assert_eq!(borrowed.read_exact(0, &mut output), Err(Failure(731)));
    assert_eq!(output, [0xe1, 0x88]);
    assert_eq!(state.reads.get(), 1);
}

#[test]
fn construction_rejects_outside_and_overflow_with_original_nondebug_source() {
    for (capacity, start, length) in [(10, 8, 3), (10, 11, 0), (u32::MAX, u32::MAX, 1)] {
        let (provider, state) = source(capacity);
        let identity = ptr::from_ref(&*provider.identity);
        let Err(failure) = ReadWindow::new(provider, start, length) else {
            panic!("out-of-range window accepted");
        };
        assert_eq!(failure.error(), &RangeError::OutOfBounds);
        assert!(format!("{failure:?}").contains("OutOfBounds"));
        let (reason, recovered) = failure.into_parts();
        assert_eq!(reason, RangeError::OutOfBounds);
        assert!(ptr::eq(ptr::from_ref(&*recovered.identity), identity));
        assert_eq!(*recovered.identity, 91);
        assert_eq!(state.capacity_calls.get(), 1);
        assert_eq!(state.reads.get(), 0);
    }
}

#[test]
fn zero_length_window_at_source_end_is_valid() {
    let (provider, state) = source(10);
    let mut reader = window(provider, 10, 0);
    assert_eq!(reader.capacity(), 0);
    assert_eq!(reader.read_exact(0, &mut []), Ok(()));
    assert_eq!(
        reader.read_exact(1, &mut []),
        Err(ReadError::Range(RangeError::OutOfBounds))
    );
    assert_eq!(
        reader.read_exact(0, &mut [0]),
        Err(ReadError::Range(RangeError::OutOfBounds))
    );
    assert_eq!(state.reads.get(), 0);
}

#[test]
fn window_maps_offset_once_and_reads_exact_length() {
    let (provider, state) = source(100);
    let mut reader = window(provider, 30, 20);
    assert_eq!(reader.capacity(), 20);
    let mut output = [0; 4];
    assert_eq!(reader.read_exact(16, &mut output), Ok(()));
    assert_eq!(output, [0x35; 4]);
    assert_eq!(state.offset.get(), 46);
    assert_eq!(state.length.get(), 4);
    assert_eq!(state.reads.get(), 1);
}

#[test]
fn local_bounds_and_overflow_fail_before_source_read() {
    let (provider, state) = source(u32::MAX);
    let mut reader = window(provider, 0, u32::MAX);
    let mut output = [0x77; 2];
    assert_eq!(
        reader.read_exact(u32::MAX, &mut output),
        Err(ReadError::Range(RangeError::OutOfBounds))
    );
    assert_eq!(output, [0x77; 2]);
    assert_eq!(state.reads.get(), 0);
    let (provider, state) = source(30);
    let mut reader = window(provider, 10, 5);
    for offset in [4, 5, 6, u32::MAX] {
        assert_eq!(
            reader.read_exact(offset, &mut output),
            Err(ReadError::Range(RangeError::OutOfBounds))
        );
        assert_eq!(output, [0x77; 2]);
    }
    assert_eq!(state.reads.get(), 0);
}

#[test]
fn mapped_range_can_end_at_u32_max_without_overflow() {
    let (provider, state) = source(u32::MAX);
    let mut reader = window(provider, u32::MAX - 3, 3);
    assert_eq!(reader.read_exact(2, &mut [0]), Ok(()));
    assert_eq!(state.offset.get(), u32::MAX - 1);
    assert_eq!(reader.read_exact(3, &mut []), Ok(()));
    assert_eq!(state.reads.get(), 1);
}

#[test]
fn shrink_rejects_affected_reads_but_keeps_surviving_prefix() {
    let (provider, state) = source(30);
    let mut reader = window(provider, 10, 10);
    state.capacity.set(15);
    let mut output = [0x77; 2];
    assert_eq!(
        reader.read_exact(4, &mut output),
        Err(ReadError::Range(RangeError::OutOfBounds))
    );
    assert_eq!(output, [0x77; 2]);
    assert_eq!(state.reads.get(), 0);
    assert_eq!(reader.read_exact(3, &mut output), Ok(()));
    assert_eq!(output, [0x35; 2]);
    assert_eq!(state.offset.get(), 13);
    assert_eq!(state.reads.get(), 1);
    assert_eq!(reader.capacity(), 10);
}

#[test]
fn empty_reads_use_logical_window_even_after_source_shrinks_below_start() {
    let (provider, state) = source(30);
    let mut reader = window(provider, 10, 10);
    state.capacity.set(0);
    for offset in [0, 5, 10] {
        assert_eq!(reader.read_exact(offset, &mut []), Ok(()));
    }
    assert_eq!(
        reader.read_exact(11, &mut []),
        Err(ReadError::Range(RangeError::OutOfBounds))
    );
    assert_eq!(state.reads.get(), 0);
    assert_eq!(state.capacity_calls.get(), 1);
    assert_eq!(
        reader.read_exact(0, &mut [0]),
        Err(ReadError::Range(RangeError::OutOfBounds))
    );
    assert_eq!(state.reads.get(), 0);
}

#[test]
fn source_growth_does_not_expand_logical_window() {
    let (provider, state) = source(20);
    let mut reader = window(provider, 10, 10);
    state.capacity.set(100);
    assert_eq!(reader.capacity(), 10);
    assert_eq!(reader.read_exact(10, &mut []), Ok(()));
    assert_eq!(
        reader.read_exact(10, &mut [0]),
        Err(ReadError::Range(RangeError::OutOfBounds))
    );
    assert_eq!(
        reader.read_exact(11, &mut []),
        Err(ReadError::Range(RangeError::OutOfBounds))
    );
    assert_eq!(state.reads.get(), 0);
}

#[test]
fn window_preserves_raw_failure_and_partial_output_without_retry() {
    let (provider, state) = source(100);
    let mut reader = window(provider, 30, 20);
    state.fail.set(true);
    let mut output = [0x88; 2];
    assert_eq!(
        reader.read_exact(2, &mut output),
        Err(ReadError::Source(Failure(731)))
    );
    assert_eq!(output, [0xe1, 0x88]);
    assert_eq!(state.offset.get(), 32);
    assert_eq!(state.reads.get(), 1);
}

#[test]
fn successful_window_extraction_preserves_source_without_any_calls() {
    let (provider, state) = source(20);
    let identity = ptr::from_ref(&*provider.identity);
    let reader = window(provider, 10, 10);
    let recovered = reader.into_inner();
    assert!(ptr::eq(ptr::from_ref(&*recovered.identity), identity));
    assert_eq!(state.reads.get(), 0);
    assert_eq!(state.capacity_calls.get(), 1);
}

#[test]
fn nested_window_over_borrowed_dyn_slice_reads_real_bytes() {
    let mut provider =
        SliceSource::new(&[10, 20, 30, 40, 50, 60]).unwrap_or_else(|e| panic!("{e:?}"));
    let dynamic: &mut dyn ReadAt<Error = RangeError> = &mut provider;
    let outer = ReadWindow::new(ReadRef::new(dynamic), 1, 4).unwrap_or_else(|e| panic!("{e:?}"));
    let mut inner = ReadWindow::new(outer, 1, 2).unwrap_or_else(|e| panic!("{e:?}"));
    let mut output = [0; 2];
    assert_eq!(inner.read_exact(0, &mut output), Ok(()));
    assert_eq!(output, [30, 40]);
    let recovered = inner.into_inner().into_inner().into_inner();
    assert_eq!(recovered.capacity(), 6);
}
