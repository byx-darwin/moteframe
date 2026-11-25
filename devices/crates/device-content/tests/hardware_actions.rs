//! Offline backend-ticket and response-first contract regressions; all ports are test-only.
#![cfg(any(feature = "gimbal", feature = "companion-radio"))]
use device_content::*;
#[derive(Debug)]
struct Access;
impl AccessPort for Access {
    fn authorize(&self, _: Scope, _: Operation) -> Result<(), Error> {
        Ok(())
    }
}
#[cfg(feature = "gimbal")]
mod gimbal {
    use super::*;
    #[derive(Debug, Default)]
    struct Port {
        calls: u32,
        polls: u32,
        cancels: u32,
        missing: bool,
        terminal: bool,
    }
    impl GimbalPort for Port {
        fn capability(&self) -> Result<GimbalCapability, Error> {
            if self.missing {
                return Err(Error::Unavailable);
            }
            Ok(GimbalCapability {
                revision: 1,
                modes: 3,
                directions: 3,
                min_duration_ms: 1,
                max_duration_ms: 10_000,
                min_angle_centi_deg: 1,
                max_angle_centi_deg: 36_000,
            })
        }
        fn submit(&mut self, _: Scope, _: GimbalRequest) -> Result<BackendTicket, Error> {
            self.calls += 1;
            BackendTicket::new(self.calls)
        }
        fn poll(&mut self, _: Scope, _: BackendTicket) -> Result<GimbalObservation, Error> {
            self.polls += 1;
            if self.terminal {
                Ok(GimbalObservation::Terminal {
                    result: GimbalResult::Completed,
                    reason: 0,
                })
            } else {
                Ok(GimbalObservation::Running)
            }
        }
        fn cancel(&mut self, _: Scope, _: BackendTicket) -> Result<(), Error> {
            self.cancels += 1;
            Ok(())
        }
    }
    fn request() -> GimbalRequest {
        GimbalRequest {
            action_id: 1,
            action: 1,
            direction: 0,
            mode: 0,
            value: 1000,
        }
    }
    #[test]
    fn test_should_reserve_gimbal_without_actuating() -> Result<(), Error> {
        let mut service = GimbalService::<4>::new(Scope::new(1, 1)?);
        let port = Port::default();
        assert_eq!(
            service.prepare(request(), &Access, &port)?,
            GimbalState::Prepared
        );
        assert_eq!(port.calls, 0);
        Ok(())
    }
    #[test]
    fn test_should_preserve_same_id_cas_and_wait_for_actual_ticket_result() -> Result<(), Error> {
        let mut service = GimbalService::<4>::new(Scope::new(1, 1)?);
        let mut port = Port::default();
        let operation = request();
        let mut encoded = [0; 11];
        assert_eq!(operation.encode(&mut encoded)?, 11);
        assert_eq!(GimbalRequest::decode(&encoded)?, operation);
        service.prepare(operation, &Access, &port)?;
        assert_eq!(service.result_event(1, &mut [0; 7]), Err(Error::State));
        assert_eq!(
            service.prepare(operation, &Access, &port)?,
            GimbalState::Prepared
        );
        let changed = GimbalRequest {
            value: 2000,
            ..operation
        };
        assert_eq!(
            service.prepare(changed, &Access, &port),
            Err(Error::Conflict)
        );
        assert_eq!(
            service.response_admitted(1, &Access, &mut port)?,
            GimbalState::Running
        );
        service.response_admitted(1, &Access, &mut port)?;
        assert_eq!(port.calls, 1);
        assert_eq!(service.poll(1, &Access, &mut port)?, GimbalState::Running);
        port.terminal = true;
        assert_eq!(
            service.poll(1, &Access, &mut port)?,
            GimbalState::Terminal {
                result: GimbalResult::Completed,
                reason: 0
            }
        );
        let polls = port.polls;
        service.poll(1, &Access, &mut port)?;
        assert_eq!(port.polls, polls);
        let mut event = [0; 7];
        assert_eq!(service.result_event(1, &mut event)?, 7);
        assert_eq!(event, [1, 0, 0, 0, 1, 0, 0]);
        assert_eq!(
            service.prepare(changed, &Access, &port),
            Err(Error::Conflict)
        );
        Ok(())
    }
    #[test]
    fn test_should_reject_missing_gimbal_and_invalid_units_or_ids() -> Result<(), Error> {
        let mut service = GimbalService::<4>::new(Scope::new(1, 1)?);
        let mut port = Port {
            missing: true,
            ..Port::default()
        };
        assert_eq!(
            service.prepare(request(), &Access, &port),
            Err(Error::Unavailable)
        );
        port.missing = false;
        for invalid in [
            GimbalRequest {
                action_id: 0,
                ..request()
            },
            GimbalRequest {
                value: 0,
                ..request()
            },
            GimbalRequest {
                value: 10_001,
                ..request()
            },
            GimbalRequest {
                action: 0,
                ..request()
            },
        ] {
            assert_eq!(
                service.prepare(invalid, &Access, &port),
                Err(Error::Invalid)
            );
        }
        assert_eq!(BackendTicket::new(0), Err(Error::Invalid));
        service.prepare(request(), &Access, &port)?;
        service.response_admitted(1, &Access, &mut port)?;
        assert_eq!(service.invalidate(&mut port)?, 1);
        assert_eq!(port.cancels, 1);
        assert_eq!(service.state(1)?, GimbalState::Unknown);
        service.response_admitted(1, &Access, &mut port)?;
        assert_eq!(port.calls, 1);
        service.replace_scope(Scope::new(1, 2)?)?;
        assert_eq!(service.state(1), Err(Error::NotFound));
        Ok(())
    }
    #[test]
    fn test_should_keep_gimbal_identity_conflicts_when_backend_becomes_unavailable()
    -> Result<(), Error> {
        let mut service = GimbalService::<4>::new(Scope::new(1, 1)?);
        let mut port = Port::default();
        service.prepare(request(), &Access, &port)?;
        port.missing = true;
        assert_eq!(
            service.prepare(request(), &Access, &port)?,
            GimbalState::Prepared
        );
        assert_eq!(
            service.prepare(
                GimbalRequest {
                    value: 2,
                    ..request()
                },
                &Access,
                &port
            ),
            Err(Error::Conflict)
        );
        assert_eq!(port.calls, 0);
        Ok(())
    }
}
#[cfg(feature = "companion-radio")]
mod radio {
    use super::*;
    #[derive(Debug, Default)]
    struct Port {
        calls: u32,
        cancels: u32,
        missing: bool,
        stable: bool,
        desired: bool,
    }
    impl CompanionRadioPort for Port {
        fn available(&self) -> bool {
            !self.missing
        }
        fn set_enabled(&mut self, _: Scope, enabled: bool) -> Result<BackendTicket, Error> {
            self.desired = enabled;
            self.calls += 1;
            BackendTicket::new(self.calls)
        }
        fn poll(&mut self, _: Scope, _: BackendTicket) -> Result<ClassicBluetooth, Error> {
            Ok(ClassicBluetooth {
                enabled: self.desired,
                state: if self.stable {
                    if self.desired { 3 } else { 1 }
                } else if self.desired {
                    2
                } else {
                    5
                },
                address: [0; 6],
                valid: false,
            })
        }
        fn cancel(&mut self, _: Scope, _: BackendTicket) -> Result<(), Error> {
            self.cancels += 1;
            Ok(())
        }
    }
    #[test]
    fn test_should_reserve_radio_switch_without_actuating() -> Result<(), Error> {
        let mut service = CompanionRadioService::new(Scope::new(1, 1)?);
        let port = Port::default();
        assert_eq!(
            service.prepare(true, &Access, &port)?,
            CompanionRadioState::Prepared
        );
        assert_eq!(port.calls, 0);
        Ok(())
    }
    #[test]
    fn test_should_keep_radio_switch_idempotent_and_wait_for_actual_readback() -> Result<(), Error>
    {
        let mut service = CompanionRadioService::new(Scope::new(1, 1)?);
        let mut port = Port::default();
        assert_eq!(service.readback(), Err(Error::NotFound));
        service.prepare_bytes(&[1], &Access, &port)?;
        assert_eq!(service.prepare(false, &Access, &port), Err(Error::Conflict));
        assert_eq!(
            service.prepare(true, &Access, &port)?,
            CompanionRadioState::Prepared
        );
        service.response_admitted(&Access, &mut port)?;
        service.response_admitted(&Access, &mut port)?;
        assert_eq!(port.calls, 1);
        assert_eq!(
            service.poll(&Access, &mut port)?,
            CompanionRadioState::Switching
        );
        assert_eq!(service.readback()?.state, 2);
        assert_eq!(service.prepare(false, &Access, &port), Err(Error::Conflict));
        port.stable = true;
        assert_eq!(
            service.poll(&Access, &mut port)?,
            CompanionRadioState::Stable
        );
        assert_eq!(
            service.prepare(true, &Access, &port)?,
            CompanionRadioState::Stable
        );
        service.response_admitted(&Access, &mut port)?;
        assert_eq!(port.calls, 1);
        service.prepare(false, &Access, &port)?;
        service.response_admitted(&Access, &mut port)?;
        assert_eq!(
            service.poll(&Access, &mut port)?,
            CompanionRadioState::Stable
        );
        assert!(!service.readback()?.enabled);
        assert_eq!(port.calls, 2);
        Ok(())
    }
    #[test]
    fn test_should_require_actual_call_radio_and_clear_scope_without_claiming_completion()
    -> Result<(), Error> {
        let mut service = CompanionRadioService::new(Scope::new(1, 1)?);
        let mut port = Port {
            missing: true,
            ..Port::default()
        };
        assert_eq!(
            service.prepare(true, &Access, &port),
            Err(Error::Unavailable)
        );
        port.missing = false;
        assert_eq!(
            service.prepare_bytes(&[2], &Access, &port),
            Err(Error::Invalid)
        );
        assert_eq!(
            service.prepare_bytes(&[1, 0], &Access, &port),
            Err(Error::Invalid)
        );
        service.prepare(true, &Access, &port)?;
        service.response_admitted(&Access, &mut port)?;
        assert_eq!(service.invalidate(&mut port)?, CompanionRadioState::Unknown);
        assert_eq!(port.cancels, 1);
        assert_eq!(service.readback(), Err(Error::NotFound));
        service.response_admitted(&Access, &mut port)?;
        assert_eq!(port.calls, 1);
        service.replace_scope(Scope::new(2, 2)?)?;
        assert_eq!(service.state(), CompanionRadioState::Uninitialized);
        Ok(())
    }
}
