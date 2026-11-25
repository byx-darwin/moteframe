//! Regression checks for the V2 presentation contract.
use wattch5_parity_ui_demo::*;
fn tap(y: u16) -> Event {
    Event::Tap { x: 20, y }
}
#[test]
fn test_should_keep_original_menu_pages_and_source_return() {
    let mut m = Model::default();
    let d = Snapshot::default();
    assert_eq!(m.appearance().locale, Locale::Zh);
    m.handle(Event::Up, &d).unwrap_or_else(|e| panic!("{e:?}"));
    m.handle(tap(100), &d).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(m.screen(), Screen::Weather);
    m.handle(Event::Right, &d)
        .unwrap_or_else(|e| panic!("{e:?}"));
    m.handle(Event::Up, &d).unwrap_or_else(|e| panic!("{e:?}"));
    m.handle(tap(20), &d).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(m.screen(), Screen::Settings);
    m.handle(Event::Up, &d).unwrap_or_else(|e| panic!("{e:?}"));
    m.handle(tap(60), &d).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(m.screen(), Screen::Language);
    m.handle(Event::Up, &d).unwrap_or_else(|e| panic!("{e:?}"));
    m.handle(Event::Select, &d)
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(m.appearance().locale, Locale::En);
    assert_eq!(m.nav_page(), 1);
    m.handle(Event::Up, &d).unwrap_or_else(|e| panic!("{e:?}"));
    m.handle(tap(100), &d).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(m.screen(), Screen::Settings);
}
#[test]
fn test_should_expose_health_chain_and_requests_without_sensor_state() {
    let d = Snapshot::default();
    let mut m = Model::default();
    let chain = [
        Screen::Activity,
        Screen::Heart,
        Screen::Glucose,
        Screen::Hba1c,
        Screen::Pressure,
        Screen::Oxygen,
        Screen::Sleep,
        Screen::Messages,
        Screen::Temperature,
        Screen::Dial,
    ];
    for s in chain {
        m.handle(Event::Left, &d)
            .unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(m.screen(), s);
    }
    m.preview(Screen::Heart, &d)
        .unwrap_or_else(|e| panic!("{e:?}"));
    let before = d;
    assert_eq!(
        m.handle(Event::Select, &d),
        Ok(Effect::Request(UiRequest::MeasureStart(MeasureKind::Heart)))
    );
    assert_eq!(d, before);
    for state in [
        MeasureState::Idle,
        MeasureState::Pending,
        MeasureState::Measuring,
        MeasureState::Result,
        MeasureState::Error,
        MeasureState::Unavailable,
    ] {
        let mut d = d;
        d.measurements[0].state = state;
        let want = if state == MeasureState::Measuring {
            UiRequest::MeasureStop(MeasureKind::Heart)
        } else {
            UiRequest::MeasureStart(MeasureKind::Heart)
        };
        assert_eq!(m.handle(Event::Select, &d), Ok(Effect::Request(want)));
    }
    m.preview(Screen::Glucose, &d)
        .unwrap_or_else(|e| panic!("{e:?}"));
    m.handle(Event::Select, &d)
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(m.screen(), Screen::MeasureTips);
    m.handle(Event::Select, &d)
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(m.screen(), Screen::MeasureConfirm);
    assert_eq!(
        m.handle(Event::Tap { x: 90, y: 110 }, &d),
        Ok(Effect::Request(UiRequest::MeasureStart(
            MeasureKind::Glucose
        )))
    );
    assert_eq!(m.screen(), Screen::Glucose);
}
#[test]
fn test_should_keep_timer_and_stopwatch_keys_request_only() {
    let mut m = Model::default();
    let mut d = Snapshot::default();
    m.preview(Screen::Timer, &d)
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(
        m.handle(Event::Tap { x: 80, y: 80 }, &d),
        Ok(Effect::Request(UiRequest::TimerPrepare { seconds: 600 }))
    );
    assert_eq!(m.screen(), Screen::TimerRun);
    assert_eq!(
        m.handle(Event::KeyRelease, &d),
        Ok(Effect::Request(UiRequest::TimerStart))
    );
    d.timer.state = TimerState::Paused;
    assert_eq!(
        m.handle(Event::KeyRelease, &d),
        Ok(Effect::Request(UiRequest::TimerResume))
    );
    d.timer.state = TimerState::Done;
    m.inject(&d).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(m.screen(), Screen::TimerDone);
    m.preview(Screen::Stopwatch, &d)
        .unwrap_or_else(|e| panic!("{e:?}"));
    d.stopwatch.state = StopwatchState::Running;
    assert_eq!(
        m.handle(Event::Tap { x: 10, y: 112 }, &d),
        Ok(Effect::Request(UiRequest::StopwatchLap))
    );
    assert_eq!(
        m.handle(Event::KeyRelease, &d),
        Ok(Effect::Request(UiRequest::StopwatchPause))
    );
    assert_eq!(d.stopwatch.centis, 1284);
}
#[test]
fn test_should_reject_invalid_snapshots_and_navigation_atomically() {
    let mut m = Model::default();
    let d = Snapshot::default();
    let before = m;
    assert_eq!(
        m.handle(Event::Tap { x: 128, y: 20 }, &d),
        Err(UiError::Coordinate)
    );
    assert_eq!(m, before);
    let mut bad = d;
    bad.progress = 101;
    assert!(m.inject(&bad).is_err());
    assert_eq!(m, before);
    for _ in 0..6 {
        m.open(Screen::About, &d)
            .unwrap_or_else(|e| panic!("{e:?}"));
    }
    let full = m;
    assert_eq!(m.open(Screen::About, &d), Err(UiError::NavigationOverflow));
    assert_eq!(m, full);
}
#[test]
fn test_should_cycle_brightness_and_preserve_blank_rows() {
    let d = Snapshot::default();
    let mut m = Model::default();
    m.preview(Screen::Brightness, &d)
        .unwrap_or_else(|e| panic!("{e:?}"));
    for _ in 0..3 {
        m.handle(Event::Up, &d).unwrap_or_else(|e| panic!("{e:?}"));
    }
    assert_eq!(m.draft(), 0);
    m.handle(Event::Down, &d)
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(m.draft(), 4);
    assert_eq!(
        m.handle(Event::Right, &d),
        Ok(Effect::Request(UiRequest::BrightnessPreview(2)))
    );
    assert_eq!(m.appearance().brightness, 2);
    m.preview(Screen::Tools, &d)
        .unwrap_or_else(|e| panic!("{e:?}"));
    m.handle(Event::Up, &d).unwrap_or_else(|e| panic!("{e:?}"));
    let before = m;
    m.handle(tap(100), &d).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(m, before);
    m.preview(Screen::Face, &d)
        .unwrap_or_else(|e| panic!("{e:?}"));
    m.handle(Event::Left, &d)
        .unwrap_or_else(|e| panic!("{e:?}"));
    m.handle(Event::Right, &d)
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(m.screen(), Screen::Face);
    assert_eq!(m.draft(), 0);
}
#[test]
fn test_should_emit_messages_read_all_without_mutating_queue() {
    let d = Snapshot::default();
    let mut m = Model::default();
    m.preview(Screen::Messages, &d)
        .unwrap_or_else(|e| panic!("{e:?}"));
    m.handle(Event::Select, &d)
        .unwrap_or_else(|e| panic!("{e:?}"));
    m.handle(Event::Select, &d)
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(m.message_index(), 1);
    assert_eq!(
        m.handle(Event::Select, &d),
        Ok(Effect::Request(UiRequest::MessagesReadAllRequested))
    );
    assert_eq!(d.messages.len(), 2);
    assert_eq!(m.screen(), Screen::Messages);
    m.preview(Screen::MessageDetail, &d)
        .unwrap_or_else(|e| panic!("{e:?}"));
    let mut empty = d;
    empty.messages = &[];
    m.inject(&empty).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(m.message_index(), 0);
    assert_eq!(m.screen(), Screen::Messages);
}
#[test]
fn test_should_restore_notice_source_and_reject_overflow_atomically() {
    let d = Snapshot::default();
    let mut m = Model::default();
    m.preview(Screen::Heart, &d)
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(m.measurement_session(), Some(MeasureKind::Heart));
    for kind in NoticeKind::ALL {
        m.show_notice(NoticeSnapshot { kind }, &d)
            .unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(m.screen(), kind.screen());
        m.handle(Event::Right, &d)
            .unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(m.screen(), Screen::Heart);
    }
    for _ in 0..5 {
        m.open(Screen::About, &d)
            .unwrap_or_else(|e| panic!("{e:?}"));
    }
    let before = m;
    assert_eq!(
        m.show_notice(
            NoticeSnapshot {
                kind: NoticeKind::Camera
            },
            &d
        ),
        Err(UiError::NavigationOverflow)
    );
    assert_eq!(m, before);
}
#[test]
fn test_should_keep_full_lap_pagination_clamped_after_shrink() {
    let laps: [u32; 20] = [100; 20];
    let mut d = Snapshot::default();
    d.stopwatch.laps = &laps;
    let mut m = Model::default();
    m.preview(Screen::Stopwatch, &d)
        .unwrap_or_else(|e| panic!("{e:?}"));
    for _ in 0..25 {
        m.handle(Event::Up, &d).unwrap_or_else(|e| panic!("{e:?}"));
    }
    assert_eq!(m.lap_top(), 18);
    d.stopwatch.laps = &[];
    m.inject(&d).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(m.lap_top(), 0);
}
#[test]
fn test_should_wrap_timer_fields_and_keep_external_workout_immutable() {
    let d = Snapshot::default();
    let mut m = Model::default();
    m.preview(Screen::TimerCustom, &d)
        .unwrap_or_else(|e| panic!("{e:?}"));
    m.handle(Event::Down, &d)
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(m.timer_hms(), [99, 5, 0]);
    m.handle(Event::Left, &d)
        .unwrap_or_else(|e| panic!("{e:?}"));
    for _ in 0..6 {
        m.handle(Event::Down, &d)
            .unwrap_or_else(|e| panic!("{e:?}"));
    }
    assert_eq!(m.timer_hms(), [99, 59, 0]);
    let mut d = d;
    d.sport.phase = SportPhase::Running;
    d.sport.locked = true;
    m.preview(Screen::SportRun, &d)
        .unwrap_or_else(|e| panic!("{e:?}"));
    let before = d;
    assert_eq!(
        m.handle(Event::Right, &d),
        Ok(Effect::Request(UiRequest::SportUnlockRequested))
    );
    assert_eq!(m.screen(), Screen::SportRun);
    assert_eq!(d, before);
}
#[test]
fn test_should_keep_unsupported_language_and_confirm_default_safe() {
    let d = Snapshot::default();
    let mut m = Model::default();
    m.preview(Screen::Language, &d)
        .unwrap_or_else(|e| panic!("{e:?}"));
    m.handle(Event::Up, &d).unwrap_or_else(|e| panic!("{e:?}"));
    m.handle(Event::Up, &d).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(
        m.handle(Event::Select, &d),
        Ok(Effect::Request(UiRequest::LanguageRequested(Language::Fr)))
    );
    assert_eq!(m.appearance().locale, Locale::Zh);
    m.preview(Screen::Power, &d)
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(m.handle(Event::Select, &d), Ok(Effect::None));
    assert_eq!(m.screen(), Screen::Dial);
}
#[test]
fn test_should_reuse_message_navigation_after_many_read_all_cycles() {
    let mut model = Model::default();
    let data = Snapshot::default();
    model
        .preview(Screen::Messages, &data)
        .unwrap_or_else(|e| panic!("{e:?}"));
    let depth = model.depth();
    for _ in 0..20 {
        for _ in 0..3 {
            model
                .handle(Event::Select, &data)
                .unwrap_or_else(|e| panic!("{e:?}"));
        }
        assert_eq!(model.screen(), Screen::Messages);
        assert_eq!(model.depth(), depth);
    }
}
#[test]
fn test_should_unwind_completed_sport_from_nested_confirmation() {
    let mut model = Model::default();
    let mut data = Snapshot::default();
    model
        .preview(Screen::Sport, &data)
        .unwrap_or_else(|e| panic!("{e:?}"));
    let depth = model.depth();
    for _ in 0..10 {
        data.sport.phase = SportPhase::Countdown;
        model
            .handle(Event::Select, &data)
            .unwrap_or_else(|e| panic!("{e:?}"));
        model
            .handle(Event::Select, &data)
            .unwrap_or_else(|e| panic!("{e:?}"));
        data.sport.phase = SportPhase::Running;
        model.inject(&data).unwrap_or_else(|e| panic!("{e:?}"));
        model
            .handle(Event::Tap { x: 10, y: 80 }, &data)
            .unwrap_or_else(|e| panic!("{e:?}"));
        model
            .handle(Event::Tap { x: 90, y: 30 }, &data)
            .unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(model.screen(), Screen::SportEnd);
        data.sport.phase = SportPhase::Done;
        model.inject(&data).unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(model.screen(), Screen::Sport);
        assert_eq!(model.depth(), depth);
    }
}
#[test]
fn test_should_unwind_message_frames_after_empty_refill_and_notice_return() {
    let mut model = Model::default();
    let mut data = Snapshot::default();
    let populated = data.messages;
    model
        .preview(Screen::Messages, &data)
        .unwrap_or_else(|e| panic!("{e:?}"));
    let depth = model.depth();
    for with_notice in [false, true] {
        for _ in 0..20 {
            model
                .handle(Event::Select, &data)
                .unwrap_or_else(|e| panic!("{e:?}"));
            if with_notice {
                model
                    .show_notice(
                        NoticeSnapshot {
                            kind: NoticeKind::Find,
                        },
                        &data,
                    )
                    .unwrap_or_else(|e| panic!("{e:?}"));
            }
            data.messages = &[];
            model.inject(&data).unwrap_or_else(|e| panic!("{e:?}"));
            if with_notice {
                model
                    .handle(Event::Right, &data)
                    .unwrap_or_else(|e| panic!("{e:?}"));
            }
            assert_eq!(model.screen(), Screen::Messages);
            assert_eq!(model.depth(), depth);
            data.messages = populated;
            model.inject(&data).unwrap_or_else(|e| panic!("{e:?}"));
        }
    }
}
#[test]
fn test_should_select_correct_timer_for_top_edge_of_lower_buttons() {
    for y in [62, 63] {
        for (x, seconds) in [(10, 300), (90, 600)] {
            let mut model = Model::default();
            let data = Snapshot::default();
            model
                .preview(Screen::Timer, &data)
                .unwrap_or_else(|e| panic!("{e:?}"));
            assert_eq!(
                model.handle(Event::Tap { x, y }, &data),
                Ok(Effect::Request(UiRequest::TimerPrepare { seconds }))
            );
        }
    }
}
