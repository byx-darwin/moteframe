//! Exercise the product layout before the first model erase or program.
use fr8029d_bringup::flash_layout::{APP, CONFIG_A, CONFIG_B, FLASH_ENV, STAGING, UpgradePlan};

#[derive(Default)]
struct Recorder {
    erases: usize,
    writes: usize,
}

impl Recorder {
    fn install(&mut self, flash: &mut [u8], package: &[u8], header: [u32; 4]) -> Result<(), ()> {
        let [length, source, destination, unit] = header;
        let plan = UpgradePlan::check(length, package.len(), source, destination, unit).ok_or(())?;
        let staged = plan.staging();
        flash[staged.start as usize..staged.end as usize].fill(0xff);
        self.erases += 1;
        let start = staged.start as usize;
        flash[start..start + package.len()].copy_from_slice(package);
        self.writes += 1;
        for offset in (0..plan.destination().len()).step_by(unit as usize) {
            let dest = destination as usize + offset;
            flash[dest..dest + unit as usize].fill(0xff);
            self.erases += 1;
            flash.copy_within(start + offset..start + offset + unit as usize, dest);
            self.writes += 1;
        }
        Ok(())
    }
}

#[test]
fn rejects_bad_plans_before_first_erase_or_write() {
    let mut flash = vec![0x5a; 1 << 20];
    let mut recorder = Recorder::default();
    for (bytes, header) in [
        (0, [0, STAGING.start, APP.start, 8192]),
        (0x3a001, [0x3a001, STAGING.start, APP.start, 8192]),
        (0x40000, [0x40000, STAGING.start, APP.start, 8192]),
        (3, [2, STAGING.start, APP.start, 8192]),
        (2, [3, STAGING.start, APP.start, 8192]),
        (1, [1, CONFIG_B.start, APP.start, 8192]),
        (1, [1, STAGING.end, APP.start, 8192]),
        (1, [1, STAGING.start, CONFIG_A.start, 8192]),
        (1, [1, STAGING.start, APP.start, 16384]),
        (1, [1, u32::MAX, APP.start, 8192]),
        (1, [u32::MAX, STAGING.start, APP.start, 8192]),
    ] {
        assert!(
            recorder
                .install(&mut flash, &vec![0; bytes], header)
                .is_err()
        );
        assert_eq!((recorder.erases, recorder.writes), (0, 0));
    }
    assert!(flash.iter().all(|&b| b == 0x5a));
}

#[test]
fn last_rounded_copy_preserves_flashenv_both_slots_and_reserved_tail() {
    for size in [1, 4097, 8193, 0x39fff, 0x3a000] {
        let mut flash = vec![0x65; 1 << 20];
        let package: Vec<u8> = (0..size).map(|i| (i % 251) as u8).collect();
        let mut recorder = Recorder::default();
        assert!(
            recorder
                .install(&mut flash, &package, [size, STAGING.start, APP.start, 8192])
                .is_ok()
        );
        let end = APP.start as usize + size as usize;
        assert_eq!(&flash[APP.start as usize..end], &package);
        let rounded = size.next_multiple_of(8192) as usize;
        assert!(
            flash[end..APP.start as usize + rounded]
                .iter()
                .all(|&b| b == 0xff)
        );
        assert!(
            flash[FLASH_ENV.start as usize..STAGING.start as usize]
                .iter()
                .all(|&b| b == 0x65)
        );
        assert!(flash[STAGING.end as usize..].iter().all(|&b| b == 0x65));
    }
}

#[test]
fn maximum_and_sdk_payload_have_known_physical_footprints() {
    for (size, stage_end, copy_end) in [(0x2d040, 0x70000, 0x30000), (0x3a000, 0x7c000, 0x3c000)] {
        let plan = UpgradePlan::check(size, size as usize, 0x42000, 0x2000, 8192);
        assert_eq!(plan.map(UpgradePlan::staging), Some(0x42000..stage_end));
        assert_eq!(plan.map(UpgradePlan::destination), Some(0x2000..copy_end));
    }
    assert!(UpgradePlan::check(u32::MAX, u32::MAX as usize, 0x42000, 0x2000, 8192).is_none());
}
