// Copyright 2026 MoteFrame contributors
// SPDX-License-Identifier: Apache-2.0
// See LICENSE and NOTICE for license and source attribution.

//! Executed fixed consumers; no hardware, product payload, heap or static state.
#![no_std]
#![allow(dead_code, unused_imports, unused_mut)]
use core::{cell::Cell, hint::black_box};

#[cfg(any(storage_owned, storage_borrowed))]
mod storage {
    use super::black_box;
    use moteframe_core::ReadAt;
    use moteframe_storage::{ReadRef, ReadWindow, SliceSource};

    #[cfg(storage_owned)]
    type Owner = ReadWindow<SliceSource<'static>>;
    #[cfg(storage_borrowed)]
    type Owner = ReadWindow<ReadRef<'static, SliceSource<'static>>>;
    #[cfg(module_layout)]
    #[unsafe(no_mangle)]
    pub static MODULE_OWNER: [u8; core::mem::size_of::<Owner>()] =
        [0; core::mem::size_of::<Owner>()];

    pub fn exercise(seed: u32) -> u32 {
        let mut bytes = [0u8; 64];
        for (index, byte) in bytes.iter_mut().enumerate() {
            *byte = (seed.wrapping_add(index as u32) & 255) as u8;
        }
        let Ok(mut source) = SliceSource::new(black_box(&bytes)) else {
            return 0;
        };
        #[cfg(storage_borrowed)]
        let source = ReadRef::new(&mut source);
        let Ok(mut view) = ReadWindow::new(source, black_box(8), black_box(32)) else {
            return 0;
        };
        let mut out = [0xee; 4];
        let result = view.read_exact(black_box(seed & 31), &mut out);
        let mut sum = if result.is_ok() { 17u32 } else { 31u32 };
        for byte in out {
            sum = sum.wrapping_mul(31).wrapping_add(u32::from(byte));
        }
        let _ = view.into_inner();
        black_box(sum)
    }
}
#[cfg(any(storage_owned, storage_borrowed))]
pub use storage::exercise;

#[cfg(any(ble8, ble32, ble_borrowed8))]
mod ble {
    use super::{Cell, black_box};
    use moteframe_ble::{CheckedNotifier, NotificationBackend, NotificationRef, WriteSlot};
    #[cfg(ble32)]
    const N: usize = 32;
    #[cfg(not(ble32))]
    const N: usize = 8;
    #[derive(Clone, Copy, PartialEq, Eq)]
    struct Target {
        generation: u32,
        attribute: u16,
        bearer: u8,
    }
    struct Backend<'a> {
        trace: &'a Cell<u32>,
        generation: u32,
        capacity: usize,
        fail: bool,
    }
    fn event(trace: &Cell<u32>, value: u32) {
        trace.set(trace.get().wrapping_mul(31).wrapping_add(value));
    }
    impl NotificationBackend for Backend<'_> {
        type Target = Target;
        type Error = u8;
        fn notification_capacity(&self, target: Target) -> Result<usize, u8> {
            event(self.trace, 11);
            if target.generation != self.generation {
                Err(1)
            } else {
                Ok(self.capacity)
            }
        }
        fn try_queue_notification(&mut self, target: Target, value: &[u8]) -> Result<(), u8> {
            event(self.trace, 13);
            if self.fail || target.generation != self.generation || value.len() > self.capacity {
                return Err(2);
            }
            for &byte in value {
                event(self.trace, u32::from(byte));
            }
            Ok(())
        }
    }
    #[cfg(not(ble_borrowed8))]
    type Owner = CheckedNotifier<Backend<'static>>;
    #[cfg(ble_borrowed8)]
    type Owner = CheckedNotifier<NotificationRef<'static, Backend<'static>>>;
    #[cfg(module_layout)]
    #[unsafe(no_mangle)]
    pub static MODULE_OWNER: [u8; core::mem::size_of::<Owner>()] =
        [0; core::mem::size_of::<Owner>()];
    #[cfg(module_layout)]
    #[unsafe(no_mangle)]
    pub static MODULE_SLOT: [u8; core::mem::size_of::<WriteSlot<Target, N>>()] =
        [0; core::mem::size_of::<WriteSlot<Target, N>>()];

    pub fn exercise(seed: u32) -> u32 {
        let trace = Cell::new(seed);
        let target = Target {
            generation: black_box(seed),
            attribute: 1,
            bearer: 0,
        };
        let mut backend = Backend {
            trace: &trace,
            generation: if seed == 2 { 0 } else { seed },
            capacity: black_box(if seed & 1 == 0 { 8 } else { 4 }),
            fail: seed == 3,
        };
        #[cfg(ble_borrowed8)]
        let backend = NotificationRef::new(&mut backend);
        let mut sender = CheckedNotifier::new(backend);
        let mut value = [0u8; 8];
        for (index, byte) in value.iter_mut().enumerate() {
            *byte = (seed.wrapping_add(index as u32) & 255) as u8;
        }
        let length = black_box((seed & 7) as usize + 1);
        event(
            &trace,
            if sender.try_notify(target, &value[..length]).is_ok() {
                17
            } else {
                19
            },
        );
        let _ = sender.into_inner();
        let mut slot = WriteSlot::<Target, N>::new();
        if slot.push(target, &value[..length]).is_err() {
            return 0;
        }
        let mut out = [0xee; 8];
        let Ok(Some(meta)) = slot.take_into(&mut out) else {
            return 0;
        };
        event(&trace, meta.target.generation);
        for byte in &out[..meta.length] {
            event(&trace, u32::from(*byte));
        }
        black_box(trace.get())
    }
}
#[cfg(any(ble8, ble32, ble_borrowed8))]
pub use ble::exercise;
