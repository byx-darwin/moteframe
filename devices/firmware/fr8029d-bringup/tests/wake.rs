//! Clock and wake regressions shared by the two board executors.
#![cfg(feature = "ble-gatt-probe")]

use fr8029d_bringup::{
    advertising::CLOCK_MASK,
    connection::PacketTime,
    wake::{RadioDeadline, SlotWatchdog, WakeSignal, next_delay_us},
};

fn clock(coarse: u32) -> PacketTime {
    PacketTime {
        coarse,
        fine_countdown: 624,
    }
}

#[test]
fn connection_service_becomes_due_at_the_lead_time_across_wrap() {
    let link = Some(RadioDeadline::Connection { start: 8, lead: 12 });
    assert_eq!(
        next_delay_us(clock(CLOCK_MASK - 6), 0, link, None),
        Some(937)
    );
    assert_eq!(next_delay_us(clock(CLOCK_MASK - 3), 0, link, None), Some(0));
    assert_eq!(next_delay_us(clock(9), 0, link, None), Some(0));
}

#[test]
fn unfinished_radio_event_keeps_its_watchdog_deadline() {
    let pending = Some(RadioDeadline::Completion {
        start: CLOCK_MASK - 2,
        timeout: 160,
    });
    assert_eq!(next_delay_us(clock(156), 0, pending, None), Some(312));
    assert_eq!(next_delay_us(clock(157), 0, pending, None), Some(0));
}

#[test]
fn a_long_slot_stays_valid_past_50_ms_across_clock_wrap() {
    let slot = SlotWatchdog::new(CLOCK_MASK - 2, 0x8000 | 0xb7);
    assert!(!slot.expired(CLOCK_MASK - 3));
    assert!(!slot.expired(157));
    assert_eq!(
        next_delay_us(clock(157), 0, Some(slot.deadline()), None),
        Some(7_812)
    );
    assert!(!slot.expired(181));
    assert!(slot.expired(182));
}

#[test]
fn a_short_advertising_slot_times_out_at_the_existing_watchdog_boundary() {
    let slot = SlotWatchdog::new(100, 0x0acf);
    assert!(!slot.expired(259));
    assert!(slot.expired(260));
}

#[test]
fn the_earliest_host_or_radio_deadline_controls_the_wait() {
    let advertising = Some(RadioDeadline::Advertising { at: 10 });
    assert_eq!(
        next_delay_us(clock(8), 500, advertising, Some(520)),
        Some(20)
    );
    assert_eq!(
        next_delay_us(clock(8), 500, advertising, Some(1200)),
        Some(625)
    );
    assert_eq!(next_delay_us(clock(8), 500, None, Some(499)), Some(0));
    assert_eq!(
        next_delay_us(clock(8), 500, None, Some(u64::MAX)),
        Some(u32::MAX)
    );
    assert_eq!(next_delay_us(clock(8), 500, None, None), None);
}

#[test]
fn wake_signalled_during_a_poll_prevents_the_next_sleep() {
    let wake = WakeSignal::new();
    assert!(!wake.take());
    wake.signal();
    assert!(wake.is_pending());
    assert!(wake.take());
    assert!(!wake.is_pending());
}

#[test]
fn completion_arriving_while_arming_nvic_prevents_blocking() {
    let wake = WakeSignal::new();
    assert!(!wake.should_wait(Some(50_000), || true));
    assert!(wake.should_wait(Some(50_000), || false));
}

#[test]
fn an_armed_watchdog_wakes_for_timeout_even_without_radio_irq() {
    let wake = WakeSignal::new();
    let pending = Some(RadioDeadline::Completion {
        start: 100,
        timeout: 160,
    });
    let delay = next_delay_us(clock(100), 0, pending, None);
    assert!(wake.should_wait(delay, || false));
    let due = next_delay_us(clock(260), 50_000, pending, None);
    assert!(!wake.should_wait(due, || panic!("watchdog already due")));
}

#[test]
fn radio_wait_accounts_for_the_partial_tick_before_the_boundary() {
    let now = PacketTime {
        coarse: CLOCK_MASK,
        fine_countdown: 124,
    };
    let advertising = Some(RadioDeadline::Advertising { at: 0 });
    assert_eq!(next_delay_us(now, 0, advertising, None), Some(62));
}
