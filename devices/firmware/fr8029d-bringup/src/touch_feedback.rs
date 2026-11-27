//! Independent idle calibration and debounced feedback for two electrodes.

/// Press detection for the two onboard capacitive electrodes.
#[derive(Debug, Default)]
pub struct TouchFeedback {
    keys: [Key; 2],
}

#[derive(Debug, Default)]
struct Key {
    reference: u32,
    last_valid_us: u32,
    calibration: u8,
    streak: u8,
    pressed: bool,
}

impl Key {
    #[inline(never)]
    fn update(&mut self, raw: u32, now_us: u32) -> bool {
        if now_us.wrapping_sub(self.last_valid_us) >= TouchFeedback::STALE_US {
            self.pressed = false;
            self.streak = 0;
        }
        if raw == TouchFeedback::PENDING {
            return self.pressed;
        }
        if raw == 0 || raw == u32::from(u16::MAX) {
            self.streak = 0;
            return self.pressed;
        }

        self.last_valid_us = now_us;
        // The sum of 16 samples is the idle mean with four fractional bits.
        if self.calibration < TouchFeedback::CALIBRATION_SCANS {
            self.reference += raw;
            self.calibration += 1;
            return false;
        }

        let baseline = self.reference >> 4;
        let scaled = raw * 100;
        let threshold = if self.pressed { 98 } else { 97 };
        let candidate = scaled < baseline * threshold;
        if candidate == self.pressed {
            self.streak = 0;
            // Follow only readings close to a confirmed idle baseline. A
            // press or a nearly pressed reading must not move the reference.
            if !self.pressed && scaled.abs_diff(baseline * 100) <= baseline {
                self.reference = self.reference + raw - baseline;
            }
        } else {
            self.streak += 1;
            if self.streak == 3 {
                self.pressed = candidate;
                self.streak = 0;
            }
        }
        self.pressed
    }
}

impl TouchFeedback {
    const PENDING: u32 = 1 << 16;
    /// Number of untouched valid scans averaged per electrode at startup.
    pub const CALIBRATION_SCANS: u8 = 16;
    /// Maximum time a pressed key can retain its last valid sample.
    pub const STALE_US: u32 = 25_000;

    /// Packed idle references for diagnostics. A zero halfword is uncalibrated.
    #[must_use]
    pub fn packed_baseline(&self) -> u32 {
        let baseline = self.keys.each_ref().map(|key| {
            if key.calibration == Self::CALIBRATION_SCANS {
                key.reference >> 4
            } else {
                0
            }
        });
        baseline[0] | (baseline[1] << 16)
    }

    /// Advance scan feedback and return a bit mask of pressed electrodes.
    ///
    /// Pass `None` while a scan is pending and zero for invalid samples.
    /// Use the low 32 bits of a monotonic microsecond clock on each poll.
    pub fn update(&mut self, raw: Option<[u16; 2]>, now_us: u32) -> u8 {
        let [first, second] = raw.map_or([Self::PENDING; 2], |values| values.map(u32::from));
        u8::from(self.keys[0].update(first, now_us))
            | (u8::from(self.keys[1].update(second, now_us)) << 1)
    }
}

#[cfg(test)]
mod tests {
    use super::TouchFeedback;

    #[derive(Default)]
    struct Scans {
        feedback: TouchFeedback,
        now_us: u32,
    }

    impl Scans {
        fn sample(&mut self, raw: [u16; 2]) -> u8 {
            self.now_us = self.now_us.wrapping_add(5_000);
            self.feedback.update(Some(raw), self.now_us)
        }
    }

    #[test]
    fn calibrates_debounces_and_keeps_feedback_on_until_both_keys_release() {
        let mut state = Scans::default();
        for _ in 0..16 {
            assert_eq!(state.sample([1000, 1200]), 0);
        }
        assert_eq!(state.sample([940, 1200]), 0);
        assert_eq!(state.sample([940, 1200]), 0);
        assert_eq!(state.sample([940, 1200]), 1);
        for _ in 0..3 {
            assert_eq!(state.sample([975, 1200]), 1);
        }
        for _ in 0..2 {
            assert_eq!(state.sample([940, 1100]), 1);
        }
        assert_eq!(state.sample([940, 1100]), 3);
        for _ in 0..2 {
            assert_eq!(state.sample([1000, 1100]), 3);
        }
        assert_eq!(state.sample([1000, 1100]), 2);
        for _ in 0..2 {
            assert_eq!(state.sample([1000, 1200]), 2);
        }
        assert_eq!(state.sample([1000, 1200]), 0);
    }

    #[test]
    fn invalid_raw_value_does_not_count_as_a_press_or_join_a_bounce() {
        let mut state = Scans::default();
        for _ in 0..16 {
            state.sample([1000, 1200]);
        }
        for value in [940, 940, 0, 940, 940] {
            assert_eq!(state.sample([value, 1200]), 0);
        }
        assert_eq!(state.sample([940, 1200]), 1);
    }

    #[test]
    fn invalid_second_channel_does_not_block_first_channel_calibration() {
        let mut state = Scans::default();
        for _ in 0..16 {
            state.sample([1000, 0]);
        }
        assert_eq!(state.feedback.packed_baseline(), 1000);
        for _ in 0..2 {
            assert_eq!(state.sample([940, 0]), 0);
        }
        assert_eq!(state.sample([940, 0]), 1);
        for _ in 0..16 {
            assert_eq!(state.sample([940, 1200]), 1);
        }
        assert_eq!(state.feedback.packed_baseline(), 1000 | (1200 << 16));
        for _ in 0..2 {
            assert_eq!(state.sample([940, 1100]), 1);
        }
        assert_eq!(state.sample([940, 1100]), 3);
    }

    #[test]
    fn invalid_channel_releases_without_releasing_healthy_key() {
        let mut state = Scans::default();
        for _ in 0..16 {
            state.sample([1000, 1200]);
        }
        for _ in 0..3 {
            state.sample([940, 1100]);
        }
        for _ in 0..10 {
            state.sample([u16::MAX, 1100]);
        }
        assert_eq!(state.sample([0, 1100]), 2);
        for _ in 0..2 {
            assert_eq!(state.sample([940, 1100]), 2);
        }
        assert_eq!(state.sample([940, 1100]), 3);
    }

    #[test]
    fn a_missing_scan_expires_a_press_at_the_deadline_across_clock_wrap() {
        let mut state = Scans {
            now_us: u32::MAX - 100_000,
            ..Scans::default()
        };
        for _ in 0..16 {
            state.sample([1000, 1200]);
        }
        for _ in 0..3 {
            state.sample([940, 1200]);
        }
        let last_valid = state.now_us;
        let deadline = last_valid.wrapping_add(TouchFeedback::STALE_US);
        assert_eq!(state.feedback.update(None, deadline.wrapping_sub(1)), 1);
        assert_eq!(state.feedback.update(None, deadline), 0);
        state.now_us = deadline;
        for _ in 0..2 {
            assert_eq!(state.sample([940, 1200]), 0);
        }
        assert_eq!(state.sample([940, 1200]), 1);
    }

    #[test]
    fn tracks_slow_idle_drift_without_absorbing_a_press() {
        let mut state = Scans::default();
        for _ in 0..16 {
            state.sample([1000, 1200]);
        }
        for raw in (950..1000).rev() {
            for _ in 0..60 {
                assert_eq!(state.sample([raw, 1200]), 0);
            }
        }
        for _ in 0..3 {
            state.sample([900, 1200]);
        }
        for _ in 0..100 {
            assert_eq!(state.sample([900, 1200]), 1);
        }
    }
}
