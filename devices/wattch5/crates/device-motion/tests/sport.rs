//! Real transition/counter bookkeeping tested with test-only hardware ports.
#![cfg(feature = "sport")]
use device_motion::{ContextAliases, MotionError, SportBackend, SportEngine, SportState};
#[derive(Debug, Default)]
struct Backend {
    calls: Vec<&'static str>,
    starts: Vec<(u64, u16, u8)>,
    available: bool,
    storage: bool,
}
impl SportBackend for Backend {
    fn start(&mut self, id: u64, sport: u16, environment: u8) -> Result<(), MotionError> {
        self.starts.push((id, sport, environment));
        self.calls.push("running");
        Ok(())
    }
    fn supports_saving(&self) -> bool {
        self.storage
    }
    fn supports(&self, sport: u16, environment: u8) -> bool {
        self.available && sport == 1 && environment == 0
    }
    fn transition(&mut self, _: u64, target: SportState) -> Result<(), MotionError> {
        self.calls.push(match target {
            SportState::Running => "running",
            SportState::Paused => "paused",
            SportState::Stopped => "stopped",
        });
        Ok(())
    }
    fn set_saving(&mut self, _: u64, enabled: bool) -> Result<bool, MotionError> {
        if !self.storage {
            return Err(MotionError::Unavailable);
        }
        Ok(enabled)
    }
}
#[test]
fn test_should_keep_active_duration_separate_from_wallclock_span_and_not_replay_transitions() {
    let mut e = SportEngine::new(1).unwrap_or_else(|e| panic!("{e:?}"));
    let mut b = Backend {
        available: true,
        storage: true,
        ..Backend::default()
    };
    let id = e
        .start(1, 0, 1000, &mut b)
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(id, 1);
    assert_eq!(e.control(id, 1, 2000, &mut b), Ok(SportState::Paused));
    assert_eq!(e.control(id, 1, 3000, &mut b), Ok(SportState::Paused));
    assert_eq!(e.active_ms(id, 5000), Ok(1000));
    assert_eq!(e.control(id, 2, 5000, &mut b), Ok(SportState::Running));
    assert_eq!(e.control(id, 3, 6000, &mut b), Ok(SportState::Stopped));
    assert_eq!(e.active_ms(id, 9000), Ok(2000));
    assert_eq!(b.calls, ["running", "paused", "running", "stopped"]);
    assert_eq!(e.control(id, 3, 10000, &mut b), Ok(SportState::Stopped));
    assert_eq!(b.calls.len(), 4);
    assert_eq!(e.control(id, 2, 10001, &mut b), Err(MotionError::Conflict));
    assert_eq!(e.save_control(id, 1, &mut b), Err(MotionError::Conflict));
    assert_eq!(e.save_control(id, 2, &mut b), Ok(false));
}
#[test]
fn test_should_preserve_u64_session_context_identity_and_reject_missing_backends() {
    let mut e = SportEngine::new(0x1_0000_0001).unwrap_or_else(|e| panic!("{e:?}"));
    let mut b = Backend::default();
    assert_eq!(e.start(1, 0, 0, &mut b), Err(MotionError::Unavailable));
    assert!(b.calls.is_empty());
    b.available = true;
    let id = e
        .start(1, 0, 100, &mut b)
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(id, 0x1_0000_0001);
    assert_eq!(e.control(1, 1, 200, &mut b), Err(MotionError::NotFound));
    assert_eq!(e.control(id, 1, 99, &mut b), Err(MotionError::Time));
    let mut map = ContextAliases::<2>::new();
    let alias = map
        .assign(e.session(id).unwrap_or_else(|e| panic!("{e:?}")))
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(alias, 1);
    assert_eq!(map.resolve(alias), Ok(id));
    assert_eq!(
        map.assign(e.session(id).unwrap_or_else(|e| panic!("{e:?}"))),
        Ok(alias)
    );
    map.session_ended(id);
    assert_eq!(map.resolve(alias), Err(MotionError::NotFound));
    assert_eq!(e.save_control(id, 1, &mut b), Err(MotionError::Unavailable));
}
#[test]
fn test_should_reject_saving_query_when_storage_is_unavailable() {
    let mut e = SportEngine::new(1).unwrap_or_else(|e| panic!("{e:?}"));
    let mut b = Backend {
        available: true,
        ..Backend::default()
    };
    let id = e.start(1, 0, 0, &mut b).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(e.save_control(id, 2, &mut b), Err(MotionError::Unavailable));
}
#[test]
fn test_should_reject_unknown_wire_sport_and_environment_before_backend() {
    let mut e = SportEngine::new(1).unwrap_or_else(|e| panic!("{e:?}"));
    let mut b = Backend {
        available: true,
        ..Backend::default()
    };
    assert_eq!(e.start(24, 0, 0, &mut b), Err(MotionError::Invalid));
    assert_eq!(e.start(1, 3, 0, &mut b), Err(MotionError::Invalid));
    assert!(b.calls.is_empty());
}
#[test]
fn test_should_stop_real_resources_even_when_wire_duration_cannot_fit() {
    let mut e = SportEngine::new(1).unwrap_or_else(|e| panic!("{e:?}"));
    let mut b = Backend {
        available: true,
        ..Backend::default()
    };
    let id = e.start(1, 0, 0, &mut b).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(
        e.control(id, 3, u64::from(u32::MAX) + 1, &mut b),
        Ok(SportState::Stopped)
    );
    assert_eq!(b.calls, ["running", "stopped"]);
    assert_eq!(
        e.active_ms(id, u64::from(u32::MAX) + 2),
        Err(MotionError::Capacity)
    );
}

#[test]
fn test_should_deliver_sport_configuration_to_actual_backend_start() {
    let mut e = SportEngine::new(7).unwrap_or_else(|e| panic!("{e:?}"));
    let mut b = Backend {
        available: true,
        ..Backend::default()
    };
    e.start(1, 0, 0, &mut b).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(b.starts, [(7, 1, 0)]);
}
