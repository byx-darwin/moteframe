//! Navigation, atomic failure and presentation-only requests.
use wattch5_retro_ui_demo::{DisplayData, Effect, Event, Model, Screen, TimerState, UiRequest};
fn send(m: &mut Model, e: Event) -> Effect {
    m.handle(e).unwrap_or_else(|e| panic!("{e:?}"))
}
#[test]
fn test_should_preserve_focus_and_cancel_or_apply_brightness() {
    let mut m = Model::default();
    send(&mut m, Event::Up);
    send(&mut m, Event::Tap { x: 10, y: 90 });
    assert_eq!(m.screen(), Screen::Settings);
    send(&mut m, Event::Select);
    assert_eq!(m.screen(), Screen::Brightness);
    assert_eq!(
        send(&mut m, Event::Up),
        Effect::Request(UiRequest::BrightnessPreview(3))
    );
    assert_eq!(
        send(&mut m, Event::Right),
        Effect::Request(UiRequest::BrightnessPreview(2))
    );
    assert_eq!(m.screen(), Screen::Settings);
    assert_eq!(m.appearance().brightness, 2);
    send(&mut m, Event::Select);
    send(&mut m, Event::Up);
    assert!(matches!(
        send(&mut m, Event::Select),
        Effect::Request(UiRequest::AppearanceCommit { .. })
    ));
    assert_eq!(m.appearance().brightness, 3);
    send(&mut m, Event::Right);
    assert_eq!(m.screen(), Screen::Tools);
    assert_eq!(m.focus(), 2);
}
#[test]
fn test_should_require_explicit_send_and_never_run_timer_or_system_actions() {
    let mut m = Model::default();
    let data = m.data();
    send(&mut m, Event::Up);
    send(&mut m, Event::Select);
    assert_eq!(
        send(&mut m, Event::Select),
        Effect::Request(UiRequest::TimerStart)
    );
    assert_eq!(m.screen(), Screen::Request);
    assert_eq!(m.data(), data);
    send(&mut m, Event::KeyLongPress);
    send(&mut m, Event::Select);
    assert_eq!(m.screen(), Screen::Confirm);
    assert_eq!(send(&mut m, Event::Select), Effect::None);
    assert_eq!(m.screen(), Screen::System);
    send(&mut m, Event::Select);
    assert_eq!(
        send(&mut m, Event::Tap { x: 90, y: 110 }),
        Effect::Request(UiRequest::Restart)
    );
    send(&mut m, Event::Right);
    assert_eq!(m.screen(), Screen::System);
    assert_eq!(m.data(), data);
}
#[test]
fn test_should_reject_invalid_coordinate_and_data_atomically() {
    let mut m = Model::default();
    let before = m;
    assert!(m.handle(Event::Tap { x: 128, y: 0 }).is_err());
    assert_eq!(m, before);
    let bad = DisplayData {
        hour: 24,
        ..DisplayData::default()
    };
    assert!(m.set_data(bad).is_err());
    assert_eq!(m, before);
    let mut data = m.data();
    data.timer_state = TimerState::Unavailable;
    m.set_data(data).unwrap_or_else(|e| panic!("{e:?}"));
    send(&mut m, Event::Up);
    send(&mut m, Event::Select);
    assert_eq!(
        send(&mut m, Event::Select),
        Effect::Request(UiRequest::TimerDataRequest)
    );
    assert_eq!(m.data(), data);
}
