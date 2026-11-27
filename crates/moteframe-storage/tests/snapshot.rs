// SPDX-License-Identifier: Apache-2.0
//! Reboot tests over the same physical bytes and a fresh RAM owner.
#![cfg(feature = "config-store")]
use core::num::NonZeroU32;
use moteframe_storage::{
    deferred::{ConfigMedia, DeferredConfig, SaveState, SaveWindow},
    snapshot::ConfigStore,
    write::{Slot, SlotRegion},
};

const SIZE: usize = 4096;
const PAGE: usize = 256;
struct Nor {
    slots: [[u8; SIZE]; 2],
    erases: usize,
    programs: usize,
    fail_after: Option<usize>,
}
impl Nor {
    fn blank() -> Self {
        Self {
            slots: [[255; SIZE]; 2],
            erases: 0,
            programs: 0,
            fail_after: None,
        }
    }
}
impl SlotRegion<PAGE> for Nor {
    type Error = ();
    fn slot_bytes(&self) -> usize {
        SIZE
    }
    fn ready(&mut self) -> Result<(), ()> {
        Ok(())
    }
    fn read(&mut self, slot: Slot, offset: usize, out: &mut [u8]) -> Result<(), ()> {
        out.copy_from_slice(&self.slots[slot as usize][offset..offset + out.len()]);
        Ok(())
    }
    fn prepare(&mut self, slot: Slot) -> Result<(), ()> {
        self.erases += 1;
        self.slots[slot as usize].fill(255);
        Ok(())
    }
    fn program_page(&mut self, slot: Slot, offset: usize, data: &[u8; PAGE]) -> Result<(), ()> {
        assert_eq!(offset % PAGE, 0);
        if self.fail_after == Some(self.programs) {
            return Err(());
        }
        self.programs += 1;
        let page = &mut self.slots[slot as usize][offset..offset + PAGE];
        assert!(page.iter().all(|&byte| byte == 255));
        page.copy_from_slice(data);
        Ok(())
    }
}
const WINDOW: SaveWindow = SaveWindow::Reserved { available_us: 100 };
fn store(nor: &mut Nor) -> ConfigStore<'_, Nor, PAGE> {
    ConfigStore::open(nor, 512, 1..=3).unwrap_or_else(|e| panic!("{e:?}"))
}
#[test]
fn reboot_selects_a_until_b_is_committed() {
    let mut nor = Nor::blank();
    let budget = NonZeroU32::new(100).unwrap_or_else(|| unreachable!());
    {
        let mut c = DeferredConfig::new(1, [1; 300], budget, store(&mut nor));
        assert_eq!(c.flush(WINDOW), Ok(SaveState::Saved));
        c.apply(2, [2; 300]);
        assert_eq!(c.active(), [2; 300]);
        for window in [
            SaveWindow::Acquiring,
            SaveWindow::Reserved { available_us: 99 },
        ] {
            assert_eq!(c.flush(window), Ok(SaveState::Pending));
        }
    }
    assert_eq!((nor.erases, nor.programs), (1, 3));
    let mut out = [0; 300];
    {
        let mut s = store(&mut nor);
        assert_eq!(s.load(&mut out), Ok(Some((1, 300))));
        assert_eq!(out, [1; 300]);
        let mut c = DeferredConfig::new(2, [2; 300], budget, s);
        assert_eq!(c.flush(WINDOW), Ok(SaveState::Saved));
    }
    assert_eq!(store(&mut nor).load(&mut out), Ok(Some((2, 300))));
    assert_eq!(out, [2; 300]);
}
#[test]
fn interrupted_updates_can_retry_with_different_bytes() {
    let mut nor = Nor::blank();
    assert_eq!(store(&mut nor).commit(1, &[1; 512]), Ok(()));
    for value in [2, 3] {
        nor.fail_after = Some(nor.programs + 1);
        assert!(store(&mut nor).commit(2, &[value; 512]).is_err());
        let mut out = [0; 512];
        assert_eq!(store(&mut nor).load(&mut out), Ok(Some((1, 512))));
        assert_eq!(out, [1; 512]);
    }
    nor.fail_after = None;
    assert_eq!(store(&mut nor).commit(2, &[4; 512]), Ok(()));
    let mut out = [0; 512];
    assert_eq!(store(&mut nor).load(&mut out), Ok(Some((2, 512))));
    assert_eq!(out, [4; 512]);
}

#[test]
fn matches_v1_golden_and_refuses_oversize_without_erasing() {
    let mut nor = Nor::blank();
    let bytes = (-125_i32).to_le_bytes();
    assert!(store(&mut nor).commit(1, &[0; 513]).is_err());
    assert_eq!((nor.erases, nor.programs), (0, 0));
    assert_eq!(store(&mut nor).commit(1, &bytes), Ok(()));
    let golden = [
        0x4d, 0x46, 0x43, 0x46, 1, 0, 28, 0, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 4, 0, 0, 0, 0xf2,
        0x5b, 0x64, 0x62, 0x83, 0xff, 0xff, 0xff,
    ];
    assert_eq!(&nor.slots[0][..32], &golden);
    assert_eq!(&nor.slots[0][SIZE - PAGE..SIZE - PAGE + 4], b"DONE");
    assert_eq!(store(&mut nor).commit(1, &bytes), Ok(()));
    assert_eq!((nor.erases, nor.programs), (1, 2));
    nor.slots[0][28] ^= 1;
    assert!(ConfigStore::open(&mut nor, 512, 1..=3).is_err());
    assert_eq!((nor.erases, nor.programs), (1, 2));
}

#[test]
fn every_page_boundary_before_done_preserves_old_snapshot() {
    for completed in 0..=3 {
        let mut nor = Nor::blank();
        assert_eq!(store(&mut nor).commit(1, &[1; 512]), Ok(()));
        nor.fail_after = Some(nor.programs + completed);
        let mut s = store(&mut nor);
        assert!(s.commit(2, &[2; 512]).is_err());
        assert_eq!(s.recover(), Ok(()));
        let mut out = [0; 512];
        assert_eq!(s.load(&mut out), Ok(Some((1, 512))));
        assert_eq!(out, [1; 512]);
        assert_eq!(store(&mut nor).load(&mut out), Ok(Some((1, 512))));
    }
}

#[test]
fn committed_unknown_version_and_partial_marker_are_preserved() {
    let mut nor = Nor::blank();
    assert_eq!(store(&mut nor).commit(1, b"old"), Ok(()));
    assert_eq!(store(&mut nor).commit(3, b"new"), Ok(()));
    let before = (nor.erases, nor.programs);
    assert!(ConfigStore::open(&mut nor, 512, 1..=2).is_err());
    nor.slots[1][SIZE - PAGE + 4] = 0;
    assert!(ConfigStore::open(&mut nor, 512, 1..=3).is_err());
    assert_eq!((nor.erases, nor.programs), before);
}

#[test]
fn equal_generation_requires_identical_payload_and_short_output_is_untouched() {
    let mut first = Nor::blank();
    assert_eq!(store(&mut first).commit(1, b"one"), Ok(()));
    first.slots[1] = first.slots[0];
    let mut short = [0xaa; 2];
    assert!(store(&mut first).load(&mut short).is_err());
    assert_eq!(short, [0xaa; 2]);
    let mut second = Nor::blank();
    assert_eq!(store(&mut second).commit(1, b"two"), Ok(()));
    first.slots[1] = second.slots[0];
    assert!(ConfigStore::open(&mut first, 512, 1..=3).is_err());
    assert_eq!((first.erases, first.programs), (1, 2));
}
