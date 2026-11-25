#[cfg(feature = "womens-health")]
#[test]
fn test_should_not_synthesize_config_or_succeed_when_atomic_backend_is_missing() {
    use device_wellness::{
        BackendError, DayNumber, Error, WomensBackend, WomensConfig, WomensMode, read_womens,
        write_womens,
    };
    struct Port {
        available: bool,
        value: Option<WomensConfig>,
        fail: bool,
    }
    impl WomensBackend for Port {
        fn available(&self) -> bool {
            self.available
        }
        fn load(&self) -> Result<Option<WomensConfig>, BackendError> {
            Ok(self.value)
        }
        fn store_atomic(&mut self, value: WomensConfig) -> Result<(), BackendError> {
            if self.fail {
                return Err(BackendError::Fault);
            }
            self.value = Some(value);
            Ok(())
        }
    }
    let mut port = Port {
        available: false,
        value: None,
        fail: false,
    };
    let value = WomensConfig {
        enabled: true,
        mode: WomensMode::Pregnancy {
            due: DayNumber::new(20000),
        },
    };
    assert_eq!(read_womens(&port), Err(Error::Unavailable));
    assert_eq!(write_womens(&mut port, value), Err(Error::Unavailable));
    assert_eq!(port.value, None);
    port.available = true;
    assert_eq!(read_womens(&port), Err(Error::NotFound));
    write_womens(&mut port, value).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(read_womens(&port), Ok(value));
    port.fail = true;
    assert_eq!(
        write_womens(
            &mut port,
            WomensConfig {
                enabled: false,
                ..value
            }
        ),
        Err(Error::Backend)
    );
    assert_eq!(read_womens(&port), Ok(value));
}
#[cfg(feature = "rri")]
#[test]
fn test_should_gate_rri_actual_start_preserve_full_parent_and_ack_only_after_admission() {
    use device_wellness::{
        BackendError, BeatClass, Error, RriBackend, RriEvent, RriObservation, RriOrigin,
        confirm_rri, poll_rri, start_rri,
    };
    struct Port {
        available: bool,
        starts: usize,
        acks: usize,
        fail_ack: bool,
        sample: Option<RriObservation>,
    }
    impl RriBackend for Port {
        fn available(&self) -> bool {
            self.available
        }
        fn origin(&self) -> RriOrigin {
            RriOrigin::DeviceAlgorithm
        }
        fn algorithm(&self) -> (&str, &str) {
            ("actual-test-detector", "test-only-version")
        }
        fn parent_is_valid(&self, parent: u64) -> bool {
            parent == 0x2_0000_0001
        }
        fn start(&mut self, _: u64, _: Option<u64>) -> Result<(), BackendError> {
            self.starts += 1;
            Ok(())
        }
        fn peek(&mut self) -> Result<Option<RriObservation>, BackendError> {
            Ok(self.sample)
        }
        fn acknowledge(&mut self, _: u64, _: u32) -> Result<(), BackendError> {
            if self.fail_ack {
                return Err(BackendError::Fault);
            }
            self.acks += 1;
            self.sample = None;
            Ok(())
        }
        fn stop(&mut self, _: u64) -> Result<(), BackendError> {
            Ok(())
        }
    }
    let mut p = Port {
        available: false,
        starts: 0,
        acks: 0,
        fail_ack: false,
        sample: None,
    };
    assert!(matches!(
        start_rri(&mut p, 1, Some(0x2_0000_0001), 0),
        Err(Error::Unavailable)
    ));
    assert_eq!(p.starts, 0);
    p.available = true;
    assert!(matches!(
        start_rri(&mut p, 1, Some(1), 0),
        Err(Error::InvalidField)
    ));
    assert_eq!(p.starts, 0);
    let mut tracker =
        start_rri(&mut p, 1, Some(0x2_0000_0001), 0).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(poll_rri(&mut p, &mut tracker, 0), Ok(None));
    p.sample = Some(RriObservation {
        event: RriEvent {
            measurement: 1,
            index: 0,
            cumulative_us: 0,
            interval_us: None,
        },
        class: BeatClass::Baseline,
    });
    let event = poll_rri(&mut p, &mut tracker, 0).unwrap_or_else(|e| panic!("{e:?}"));
    assert!(event.is_some());
    assert_eq!(poll_rri(&mut p, &mut tracker, 0), Ok(event));
    p.fail_ack = true;
    assert_eq!(confirm_rri(&mut p, &mut tracker, 1, 0), Err(Error::Backend));
    assert!(tracker.pending().is_some());
    assert!(p.sample.is_some());
    p.fail_ack = false;
    confirm_rri(&mut p, &mut tracker, 1, 0).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(confirm_rri(&mut p, &mut tracker, 1, 0), Ok(()));
    assert_eq!(p.acks, 1);
    assert_eq!(tracker.pending(), None);
}
#[cfg(feature = "uv")]
#[test]
fn test_should_not_fake_uv_value_or_release_actual_sample_before_event_and_backend_admission() {
    use device_wellness::{
        BackendError, Error, Timestamp, UvBackend, UvEngine, UvPolicy, UvProvenance, UvReading,
        UvSample, UvSource, UvValueOrigin,
    };
    struct Port {
        available: bool,
        fail_ack: bool,
        acks: usize,
        sample: Option<UvSample>,
    }
    impl UvBackend for Port {
        fn available(&self) -> bool {
            self.available
        }
        fn provenance(&self) -> UvProvenance<'_> {
            UvProvenance {
                origin: UvValueOrigin::Sensor,
                provider_id: "actual-test-provider",
                provider_version: "test-only-version",
            }
        }
        fn peek(&mut self) -> Result<Option<UvSample>, BackendError> {
            Ok(self.sample)
        }
        fn acknowledge(&mut self, _: u64) -> Result<(), BackendError> {
            if self.fail_ack {
                return Err(BackendError::Fault);
            }
            self.acks += 1;
            self.sample = None;
            Ok(())
        }
    }
    let mut p = Port {
        available: false,
        fail_ack: false,
        acks: 0,
        sample: None,
    };
    let mut engine = UvEngine::<2>::new(
        UvPolicy {
            report_enabled: true,
            alarm_enabled: false,
            threshold_x100: 0,
            interval_s: 1,
        },
        1,
    )
    .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(engine.poll(&mut p), Err(Error::Unavailable));
    assert_eq!(engine.latest(), Err(Error::NotFound));
    p.available = true;
    assert_eq!(engine.poll(&mut p), Ok(false));
    assert_eq!(engine.latest(), Err(Error::NotFound));
    p.sample = Some(UvSample {
        sample_id: 1,
        monotonic_ms: 0,
        timestamp: Timestamp::unknown(),
        source: UvSource::DeviceSampling,
        reading: UvReading {
            index_x100: Some(0),
            intensity_uw_cm2: None,
        },
    });
    engine.poll(&mut p).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(
        engine.latest().map(|value| value.reading.index_x100),
        Ok(Some(0))
    );
    assert_eq!(engine.confirm_sample(&mut p, 1), Err(Error::Busy));
    assert_eq!(p.acks, 0);
    engine.confirm_report(1).unwrap_or_else(|e| panic!("{e:?}"));
    p.fail_ack = true;
    assert_eq!(engine.confirm_sample(&mut p, 1), Err(Error::Backend));
    assert!(p.sample.is_some());
    p.fail_ack = false;
    engine
        .confirm_sample(&mut p, 1)
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(engine.confirm_sample(&mut p, 1), Ok(()));
    assert_eq!(p.acks, 1);
}
