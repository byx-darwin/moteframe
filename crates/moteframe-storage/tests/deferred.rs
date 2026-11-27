// SPDX-License-Identifier: Apache-2.0
//! Offline deferred-save acceptance tests.
use core::num::NonZeroU32;
use moteframe_storage::deferred::{ConfigMedia, DeferredConfig, SaveState, SaveWindow};
use std::cell::RefCell;

#[derive(Default)]
struct Media {
    record: Option<(u32, Vec<u8>)>,
    erases: usize,
    programs: usize,
    corrupt: bool,
    fail_read: bool,
}

struct Backend<'a>(&'a RefCell<Media>);

impl ConfigMedia for Backend<'_> {
    type Error = ();
    fn selected(&mut self) -> Result<Option<(u32, usize)>, ()> {
        Ok(self
            .0
            .borrow()
            .record
            .as_ref()
            .map(|(version, bytes)| (*version, bytes.len())))
    }
    fn read_selected(&mut self, offset: usize, out: &mut [u8]) -> Result<(), ()> {
        let media = self.0.borrow();
        if media.fail_read {
            return Err(());
        }
        let (_, data) = media.record.as_ref().ok_or(())?;
        out.copy_from_slice(&data[offset..offset + out.len()]);
        Ok(())
    }
    fn commit(&mut self, version: u32, bytes: &[u8]) -> Result<(), ()> {
        let mut media = self.0.borrow_mut();
        media.erases += 1;
        media.programs += 1;
        let mut value = bytes.to_vec();
        if media.corrupt
            && let Some(last) = value.last_mut()
        {
            *last ^= 1;
        }
        media.record = Some((version, value));
        Ok(())
    }
}
const BUDGET: NonZeroU32 = match NonZeroU32::new(100) {
    Some(value) => value,
    None => unreachable!(),
};
const OPEN: SaveWindow = SaveWindow::Reserved { available_us: 100 };

#[test]
fn active_changes_immediately_and_only_latest_is_written() {
    let media = RefCell::new(Media::default());
    let mut config = DeferredConfig::new(1, [1; 40], BUDGET, Backend(&media));
    assert_eq!(config.flush(OPEN), Ok(SaveState::Saved));
    config.apply(1, [2; 40]);
    assert_eq!(config.active(), &[2; 40]);
    config.apply(2, [3; 40]);
    assert_eq!(config.active(), &[3; 40]);
    for window in [
        SaveWindow::Acquiring,
        SaveWindow::Reserved { available_us: 99 },
    ] {
        assert_eq!(config.flush(window), Ok(SaveState::Pending));
        assert_eq!((media.borrow().erases, media.borrow().programs), (1, 1));
        assert_eq!(media.borrow().record, Some((1, vec![1; 40])));
    }
    assert_eq!(config.flush(OPEN), Ok(SaveState::Saved));
    assert_eq!(media.borrow().record, Some((2, vec![3; 40])));
    assert_eq!(config.flush(OPEN), Ok(SaveState::Saved));
    assert_eq!((media.borrow().erases, media.borrow().programs), (2, 2));
}

#[test]
fn last_byte_and_actual_readback_are_required() {
    let media = RefCell::new(Media {
        corrupt: true,
        ..Media::default()
    });
    let mut config = DeferredConfig::new(5, [4; 65], BUDGET, Backend(&media));
    assert_eq!(config.flush(OPEN), Ok(SaveState::Pending));
    assert_eq!(config.state(), SaveState::Pending);
    media.borrow_mut().fail_read = true;
    assert_eq!(config.flush(OPEN), Err(()));
    assert_eq!(media.borrow().programs, 1);
    assert_eq!(config.state(), SaveState::Pending);
    media.borrow_mut().fail_read = false;
    media.borrow_mut().corrupt = false;
    assert_eq!(config.flush(OPEN), Ok(SaveState::Saved));
}

#[test]
fn previously_committed_matching_snapshot_needs_no_write() {
    let media = RefCell::new(Media {
        record: Some((7, vec![8; 3])),
        ..Media::default()
    });
    let mut config = DeferredConfig::new(7, [8; 3], BUDGET, Backend(&media));
    assert_eq!(config.flush(OPEN), Ok(SaveState::Saved));
    assert_eq!((media.borrow().erases, media.borrow().programs), (0, 0));
}

#[test]
fn read_failure_after_commit_stays_pending_then_confirms_without_rewriting() {
    let media = RefCell::new(Media {
        fail_read: true,
        ..Media::default()
    });
    let mut config = DeferredConfig::new(0x10001, [42; 33], BUDGET, Backend(&media));
    assert_eq!(config.flush(OPEN), Err(()));
    assert_eq!(config.state(), SaveState::Pending);
    assert_eq!(config.active(), &[42; 33]);
    media.borrow_mut().fail_read = false;
    assert_eq!(config.flush(OPEN), Ok(SaveState::Saved));
    assert_eq!((media.borrow().erases, media.borrow().programs), (1, 1));
}
