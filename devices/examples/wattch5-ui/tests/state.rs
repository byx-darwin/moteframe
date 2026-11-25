//! Presentation behavior against independent literal fixtures.
use wattch5_ui_demo::{Action, Destination, Event, Model, Page};
fn brightness() -> Model {
    let mut model = Model::new(10, 50, 3).unwrap_or_else(|e| panic!("{e:?}"));
    for e in [
        Event::Up,
        Event::Up,
        Event::Tap { x: 10, y: 20 },
        Event::Tap { x: 90, y: 110 },
    ] {
        model.handle(e).unwrap_or_else(|e| panic!("{e:?}"));
    }
    assert_eq!(model.page(), Page::Brightness);
    model
}
#[test]
fn test_should_follow_menu_settings_brightness_route_and_preserve_parent() {
    let mut model = brightness();
    assert_eq!(model.level(), 3);
    assert_eq!(model.handle(Event::Up), Ok(Action::PreviewBrightness(4)));
    assert_eq!(model.handle(Event::Up), Ok(Action::PreviewBrightness(0)));
    assert_eq!(model.handle(Event::Down), Ok(Action::PreviewBrightness(4)));
    assert_eq!(model.handle(Event::Right), Ok(Action::PreviewBrightness(3)));
    assert_eq!(model.page(), Page::Settings);
    assert_eq!(model.handle(Event::Right), Ok(Action::None));
    assert_eq!(model.page(), Page::Menu);
    assert_eq!(model.menu_index(), 1);
}
#[test]
fn test_should_commit_only_demo_brightness_and_cancel_with_key() {
    let mut model = brightness();
    model.handle(Event::Up).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(
        model.handle(Event::Tap { x: 127, y: 127 }),
        Ok(Action::None)
    );
    assert_eq!(
        model.handle(Event::Tap { x: 127, y: 126 }),
        Ok(Action::CommitBrightness(4))
    );
    assert_eq!(model.page(), Page::Settings);
    model
        .handle(Event::Tap { x: 1, y: 85 })
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(model.level(), 4);
    model
        .handle(Event::Down)
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(
        model.handle(Event::KeyRelease),
        Ok(Action::PreviewBrightness(4))
    );
    assert_eq!(model.page(), Page::Dial);
}
#[test]
fn test_should_bound_inputs_and_missing_menu_rows_without_fake_business_pages() {
    assert!(Model::new(24, 0, 0).is_err());
    assert!(Model::new(0, 60, 0).is_err());
    assert!(Model::new(0, 0, 5).is_err());
    let mut model = Model::new(0, 0, 0).unwrap_or_else(|e| panic!("{e:?}"));
    let old = model;
    assert!(model.handle(Event::Tap { x: 128, y: 0 }).is_err());
    assert_eq!(model, old);
    model.handle(Event::Up).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(
        model.handle(Event::Tap { x: 20, y: 42 }),
        Ok(Action::Open(Destination::Countdown))
    );
    assert_eq!(model.page(), Page::Menu);
    model.handle(Event::Up).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(model.handle(Event::Tap { x: 20, y: 85 }), Ok(Action::None));
    model.handle(Event::Up).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(model.menu_index(), 1);
    model
        .handle(Event::Tap { x: 20, y: 42 })
        .unwrap_or_else(|e| panic!("{e:?}"));
    model.handle(Event::Up).unwrap_or_else(|e| panic!("{e:?}"));
    model.handle(Event::Up).unwrap_or_else(|e| panic!("{e:?}"));
    model.handle(Event::Up).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(model.settings_index(), 2);
    assert_eq!(model.handle(Event::Tap { x: 20, y: 85 }), Ok(Action::None));
}
