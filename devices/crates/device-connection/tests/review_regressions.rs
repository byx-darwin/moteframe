//! Independent integration review regressions retained as permanent tests.
#![allow(clippy::unwrap_used)]
#[cfg(test)]
mod tests {
    use device_connection::{ControlConnection, Receive};
    use device_protocol::{
        Frame, Header, Kind, ModuleEntry, Progress, ProtocolError, ProtocolLimits, Reassembler,
    };
    use device_services::{
        BuildProfile, Config, DeviceInfo, HandlerTable, ServiceHandler, Session, SessionError,
    };
    const ENTRIES: [ModuleEntry; 2] = [
        ModuleEntry {
            module: 0,
            major: 1,
            minor: 2,
            availability: 1,
            features: 0,
            input_max: 512,
            output_max: 512,
        },
        ModuleEntry {
            module: 2,
            major: 1,
            minor: 2,
            availability: 1,
            features: 0,
            input_max: 512,
            output_max: 512,
        },
    ];
    fn config() -> Config<'static> {
        Config {
            session: 1,
            capability_rev: 1,
            profile: BuildProfile::Commercial,
            info: DeviceInfo {
                product_id: 5,
                fw_major: 0,
                fw_minor: 1,
                fw_patch: 0,
                build_id: 1,
            },
            modules: &ENTRIES,
            limits: ProtocolLimits {
                rx_frame_max: 20,
                tx_frame_max: 20,
                control_max: 512,
                data_max: 0,
                idle_ms: 2000,
                fragment_ms: 100,
                response_ms: 10000,
            },
        }
    }
    struct H {
        allowed: bool,
        calls: u32,
    }
    impl ServiceHandler for H {
        fn required_features(&self, e: &ModuleEntry, op: u8) -> Option<u32> {
            (e.module == 2 && op == 0x30).then_some(0)
        }
        fn authorized(&self, _: Frame<'_>) -> bool {
            self.allowed
        }
        fn dispatch(&mut self, _: Frame<'_>, out: &mut [u8]) -> Result<usize, SessionError> {
            self.calls += 1;
            out[..4].fill(0);
            Ok(4)
        }
    }
    const HELLO: [u8; 18] = [0x10, 0, 1, 0, 1, 0, 0, 0, 8, 0, 1, 1, 128, 0, 0, 0, 3, 0];
    const ACTION: [u8; 10] = [0x10, 2, 0x30, 0, 2, 0, 0, 0, 0, 0];
    fn active(h: &mut impl ServiceHandler) -> ControlConnection<'static> {
        let mut c = ControlConnection::new(config()).unwrap();
        c.notifications_enabled().unwrap();
        c.receive(&HELLO, 0, h).unwrap();
        let mut b = [0; 20];
        while c.response_pending() {
            c.next_frame(h, &mut b).unwrap();
            c.admit_frame(h).unwrap();
        }
        c
    }
    #[test]
    fn negotiated_smaller_control_limit_must_work_through_handler_table() {
        let mut h = H {
            allowed: true,
            calls: 0,
        };
        let mut handlers: [&mut dyn ServiceHandler; 1] = [&mut h];
        let mut table = HandlerTable::new(&ENTRIES, &mut handlers).unwrap();
        let mut c = active(&mut table);
        assert_eq!(
            c.receive(&ACTION, 1, &mut table),
            Ok(Receive::ResponseReady)
        );
    }
    #[test]
    fn initially_denied_request_must_send_four_byte_access_denied() {
        let mut h = H {
            allowed: false,
            calls: 0,
        };
        let mut c = active(&mut h);
        assert_eq!(c.receive(&ACTION, 1, &mut h), Ok(Receive::ResponseReady));
        let mut out = [0xa5; 20];
        assert_eq!(c.next_frame(&mut h, &mut out), Ok(14));
        assert_eq!(&out[10..14], &[5, 0, 3, 255]);
        assert_eq!(h.calls, 0);
    }
    #[test]
    fn denied_response_exact_session_retry_should_remain_denied_without_false_revocation() {
        let mut h = H {
            allowed: false,
            calls: 0,
        };
        let mut s = Session::new(config()).unwrap();
        s.control_notifications_enabled().unwrap();
        let hello = Frame::decode(&HELLO, 20, 512).unwrap();
        let mut out = [0; 512];
        s.dispatch_with_handler(hello, &mut h, &mut out).unwrap();
        let action = Frame::decode(&ACTION, 20, 512).unwrap();
        assert_eq!(s.dispatch_with_handler(action, &mut h, &mut out), Ok(4));
        assert_eq!(&out[..4], &[5, 0, 3, 255]);
        assert_eq!(s.dispatch_with_handler(action, &mut h, &mut out), Ok(4));
    }
    #[test]
    fn silent_time_advances_must_reject_backwards_observation() {
        let mut r = Reassembler::<128>::new(20, 128).unwrap();
        let payload = [0u8; 10];
        let first = Frame {
            header: Header {
                kind: Kind::Request,
                module: 2,
                opcode: 0x30,
                message_id: 1,
                offset: 0,
                total_length: 20,
            },
            payload: &payload,
        };
        assert_eq!(r.push(first, 0), Ok(Progress::Pending));
        assert_eq!(r.advance(1000), Ok(()));
        assert_eq!(r.advance(999), Err(ProtocolError::InvalidState));
    }
}

#[test]
fn test_should_expose_only_actual_active_negotiated_limits_for_event_queues() {
    use device_services::ServiceHandler;
    let mut handler = tests_support::NoHandler;
    let mut connection = tests_support::connection();
    assert!(connection.active_limits().is_none());
    connection.notifications_enabled().unwrap();
    connection
        .receive(
            &[0x10, 0, 1, 0, 1, 0, 0, 0, 8, 0, 1, 1, 128, 0, 0, 0, 3, 0],
            0,
            &mut handler,
        )
        .unwrap();
    assert_eq!(
        connection.active_limits().map(|limits| limits.control_max),
        Some(128)
    );
    assert_eq!(handler.connection_closed(), Ok(()));
    connection.close(&mut handler).unwrap();
    assert!(connection.active_limits().is_none());
}
mod tests_support {
    use device_protocol::{Frame, ModuleEntry, ProtocolLimits};
    use device_services::{BuildProfile, Config, DeviceInfo, ServiceHandler, SessionError};
    pub struct NoHandler;
    impl ServiceHandler for NoHandler {
        fn required_features(&self, _: &ModuleEntry, _: u8) -> Option<u32> {
            None
        }
        fn authorized(&self, _: Frame<'_>) -> bool {
            false
        }
        fn dispatch(&mut self, _: Frame<'_>, _: &mut [u8]) -> Result<usize, SessionError> {
            Err(SessionError::InvalidHandlerResponse)
        }
    }
    pub fn connection() -> device_connection::ControlConnection<'static> {
        static ENTRIES: [ModuleEntry; 1] = [ModuleEntry {
            module: 0,
            major: 1,
            minor: 2,
            availability: 1,
            features: 0,
            input_max: 512,
            output_max: 512,
        }];
        device_connection::ControlConnection::new(Config {
            session: 1,
            capability_rev: 1,
            profile: BuildProfile::Commercial,
            info: DeviceInfo {
                product_id: 0,
                fw_major: 0,
                fw_minor: 1,
                fw_patch: 0,
                build_id: 0,
            },
            modules: &ENTRIES,
            limits: ProtocolLimits {
                rx_frame_max: 20,
                tx_frame_max: 20,
                control_max: 512,
                data_max: 0,
                idle_ms: 2000,
                fragment_ms: 100,
                response_ms: 10000,
            },
        })
        .unwrap()
    }
}
