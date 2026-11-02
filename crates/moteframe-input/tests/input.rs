//! Thresholds, normalized input, rollback on queue pressure and monotonic time.
#![allow(clippy::unwrap_used)]
use moteframe_input::{
    Button, ButtonConfig, ButtonEvent, Direction, EventQueue, InputError, Point, Touch,
    TouchConfig, TouchEvent, TouchSample,
};

fn button() -> Button {
    Button::new(ButtonConfig::new(30, Some(100)).unwrap())
}
fn touch() -> Touch {
    Touch::new(TouchConfig::new(320, 240, 5, 30, 100).unwrap())
}
fn p(x: u16, y: u16) -> Point {
    Point { x, y }
}

#[test]
fn queue_is_fifo_and_full_never_overwrites_even_at_zero_capacity() {
    let mut q = EventQueue::<u8, 2>::new();
    q.push(11).unwrap();
    q.push(12).unwrap();
    assert_eq!(q.push(13), Err(InputError::Full));
    assert_eq!(q.pop(), Some(11));
    q.push(13).unwrap();
    assert_eq!(q.pop(), Some(12));
    assert_eq!(q.pop(), Some(13));
    assert_eq!(q.pop(), None);
    assert_eq!(EventQueue::<u8, 0>::new().push(1), Err(InputError::Full));
}

#[test]
fn button_bounce_and_exact_debounce_deadline() {
    let mut b = button();
    let mut q = EventQueue::<ButtonEvent, 4>::new();
    b.sample(true, 10, &mut q).unwrap();
    b.sample(false, 20, &mut q).unwrap();
    b.sample(true, 30, &mut q).unwrap();
    b.sample(true, 59, &mut q).unwrap();
    assert_eq!(q.pop(), None);
    b.sample(true, 60, &mut q).unwrap();
    assert_eq!(q.pop(), Some(ButtonEvent::Pressed));
    b.sample(false, 70, &mut q).unwrap();
    b.sample(false, 100, &mut q).unwrap();
    assert_eq!(q.pop(), Some(ButtonEvent::Released));
}

#[test]
fn button_hold_is_once_per_press_and_can_be_disabled() {
    let mut b = button();
    let mut q = EventQueue::<ButtonEvent, 4>::new();
    b.sample(true, 0, &mut q).unwrap();
    b.sample(true, 30, &mut q).unwrap();
    assert_eq!(q.pop(), Some(ButtonEvent::Pressed));
    b.sample(true, 129, &mut q).unwrap();
    assert_eq!(q.pop(), None);
    b.sample(true, 130, &mut q).unwrap();
    b.sample(true, 500, &mut q).unwrap();
    assert_eq!(q.pop(), Some(ButtonEvent::Held));
    assert_eq!(q.pop(), None);
    let mut b = Button::new(ButtonConfig::new(0, None).unwrap());
    b.sample(true, 0, &mut q).unwrap();
    b.sample(true, u64::MAX, &mut q).unwrap();
    assert_eq!(q.pop(), Some(ButtonEvent::Pressed));
    assert_eq!(q.pop(), None);
}

#[test]
fn confirmed_release_wins_over_due_hold_and_next_press_can_hold_again() {
    for released_at in [100, 101] {
        let mut b = Button::new(ButtonConfig::new(0, Some(100)).unwrap());
        let mut q = EventQueue::<ButtonEvent, 1>::new();
        b.sample(true, 0, &mut q).unwrap();
        assert_eq!(q.pop(), Some(ButtonEvent::Pressed));
        b.sample(false, released_at, &mut q).unwrap();
        assert_eq!(q.pop(), Some(ButtonEvent::Released));
        b.sample(false, released_at + 1, &mut q).unwrap();
        assert_eq!(q.pop(), None);
        b.sample(true, released_at + 2, &mut q).unwrap();
        assert_eq!(q.pop(), Some(ButtonEvent::Pressed));
        b.sample(true, released_at + 102, &mut q).unwrap();
        assert_eq!(q.pop(), Some(ButtonEvent::Held));
    }
}

#[test]
fn button_full_queue_preserves_press_hold_and_release_for_retry() {
    let mut b = Button::new(ButtonConfig::new(0, Some(10)).unwrap());
    let mut q = EventQueue::<ButtonEvent, 1>::new();
    q.push(ButtonEvent::Released).unwrap();
    assert_eq!(b.sample(true, 0, &mut q), Err(InputError::Full));
    q.pop();
    b.sample(true, 0, &mut q).unwrap();
    assert_eq!(b.sample(true, 10, &mut q), Err(InputError::Full));
    assert_eq!(q.pop(), Some(ButtonEvent::Pressed));
    b.sample(true, 10, &mut q).unwrap();
    assert_eq!(b.sample(false, 11, &mut q), Err(InputError::Full));
    assert_eq!(q.pop(), Some(ButtonEvent::Held));
    b.sample(false, 11, &mut q).unwrap();
    assert_eq!(q.pop(), Some(ButtonEvent::Released));
}

#[test]
fn button_clock_regression_and_zero_queue_do_not_consume_transitions() {
    let mut b = Button::new(ButtonConfig::new(0, None).unwrap());
    let mut empty = EventQueue::<ButtonEvent, 0>::new();
    assert_eq!(b.sample(true, 100, &mut empty), Err(InputError::Full));
    let mut q = EventQueue::<ButtonEvent, 2>::new();
    b.sample(true, 100, &mut q).unwrap();
    assert_eq!(
        b.sample(false, 99, &mut q),
        Err(InputError::ClockRegression)
    );
    assert_eq!(q.pop(), Some(ButtonEvent::Pressed));
    b.sample(false, 100, &mut q).unwrap();
    assert_eq!(q.pop(), Some(ButtonEvent::Released));
}

#[test]
fn touch_tap_and_each_swipe_direction_use_configured_logical_coordinates() {
    for (end, want) in [
        (p(101, 101), TouchEvent::Tap(p(100, 100))),
        (p(140, 100), TouchEvent::Swipe(Direction::Right)),
        (p(60, 100), TouchEvent::Swipe(Direction::Left)),
        (p(100, 140), TouchEvent::Swipe(Direction::Down)),
        (p(100, 60), TouchEvent::Swipe(Direction::Up)),
    ] {
        let mut t = touch();
        let mut q = EventQueue::<TouchEvent, 1>::new();
        t.update(TouchSample::Down(p(100, 100)), 0, &mut q).unwrap();
        t.update(TouchSample::Up(end), 20, &mut q).unwrap();
        assert_eq!(q.pop(), Some(want));
        assert_eq!(q.pop(), None);
    }
}

#[test]
fn touch_move_away_and_back_cannot_turn_into_a_tap_or_hold() {
    let mut t = touch();
    let mut q = EventQueue::<TouchEvent, 2>::new();
    t.update(TouchSample::Down(p(10, 10)), 0, &mut q).unwrap();
    t.update(TouchSample::Move(p(30, 10)), 1, &mut q).unwrap();
    t.update(TouchSample::Move(p(10, 10)), 2, &mut q).unwrap();
    t.tick(100, &mut q).unwrap();
    t.update(TouchSample::Up(p(10, 10)), 101, &mut q).unwrap();
    assert_eq!(q.pop(), None);
}

#[test]
fn touch_hold_is_once_and_up_does_not_emit_a_second_gesture() {
    let mut t = touch();
    let mut q = EventQueue::<TouchEvent, 2>::new();
    t.update(TouchSample::Down(p(10, 10)), 0, &mut q).unwrap();
    t.tick(99, &mut q).unwrap();
    assert_eq!(q.pop(), None);
    t.tick(100, &mut q).unwrap();
    t.tick(150, &mut q).unwrap();
    t.update(TouchSample::Up(p(10, 10)), 151, &mut q).unwrap();
    assert_eq!(q.pop(), Some(TouchEvent::Held(p(10, 10))));
    assert_eq!(q.pop(), None);
}

#[test]
fn touch_up_can_complete_hold_without_a_timer_tick() {
    let mut t = touch();
    let mut q = EventQueue::<TouchEvent, 1>::new();
    t.update(TouchSample::Down(p(10, 10)), 0, &mut q).unwrap();
    t.update(TouchSample::Up(p(10, 10)), 100, &mut q).unwrap();
    assert_eq!(q.pop(), Some(TouchEvent::Held(p(10, 10))));
}

#[test]
fn touch_full_preserves_release_hold_and_cancel_for_retry() {
    for action in [TouchSample::Up(p(10, 10)), TouchSample::Cancel] {
        let mut t = touch();
        let mut q = EventQueue::<TouchEvent, 1>::new();
        t.update(TouchSample::Down(p(10, 10)), 0, &mut q).unwrap();
        q.push(TouchEvent::Cancelled).unwrap();
        assert_eq!(t.update(action, 5, &mut q), Err(InputError::Full));
        q.pop();
        t.update(action, 5, &mut q).unwrap();
        assert_eq!(
            q.pop(),
            Some(if action == TouchSample::Cancel {
                TouchEvent::Cancelled
            } else {
                TouchEvent::Tap(p(10, 10))
            })
        );
    }
    let mut t = touch();
    let mut q = EventQueue::<TouchEvent, 1>::new();
    t.update(TouchSample::Down(p(10, 10)), 0, &mut q).unwrap();
    q.push(TouchEvent::Cancelled).unwrap();
    assert_eq!(t.tick(100, &mut q), Err(InputError::Full));
    q.pop();
    t.tick(100, &mut q).unwrap();
    assert_eq!(q.pop(), Some(TouchEvent::Held(p(10, 10))));
}

#[test]
fn touch_invalid_input_and_regression_leave_the_contact_intact() {
    let mut t = touch();
    let mut q = EventQueue::<TouchEvent, 2>::new();
    assert_eq!(
        t.update(TouchSample::Move(p(1, 1)), 0, &mut q),
        Err(InputError::Sequence)
    );
    t.update(TouchSample::Down(p(10, 10)), 10, &mut q).unwrap();
    assert_eq!(
        t.update(TouchSample::Down(p(11, 11)), 11, &mut q),
        Err(InputError::Sequence)
    );
    assert_eq!(
        t.update(TouchSample::Up(p(320, 10)), 12, &mut q),
        Err(InputError::OutOfBounds)
    );
    assert_eq!(t.tick(9, &mut q), Err(InputError::ClockRegression));
    t.update(TouchSample::Up(p(10, 10)), 20, &mut q).unwrap();
    assert_eq!(q.pop(), Some(TouchEvent::Tap(p(10, 10))));
}

#[test]
fn configurations_reject_ambiguous_thresholds_and_debug_omits_events() {
    assert_eq!(
        ButtonConfig::new(0, Some(0)),
        Err(InputError::InvalidConfig)
    );
    assert_eq!(
        TouchConfig::new(0, 10, 1, 2, 10),
        Err(InputError::InvalidConfig)
    );
    assert_eq!(
        TouchConfig::new(10, 10, 2, 2, 10),
        Err(InputError::InvalidConfig)
    );
    assert_eq!(
        TouchConfig::new(10, 10, 1, 2, 0),
        Err(InputError::InvalidConfig)
    );
    let mut q = EventQueue::<u32, 1>::new();
    q.push(987_654_321).unwrap();
    assert!(!format!("{q:?}").contains("987654321"));
}
