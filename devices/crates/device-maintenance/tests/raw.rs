//! Real-port acquisition accounting and transport admission invariants.
#![cfg(feature = "raw-data")]
#![allow(clippy::unwrap_used, clippy::expect_used)]
use device_maintenance::{Authority, Capability, Error, Profile, raw::*};
use std::cell::Cell;
struct Auth(Cell<bool>);
impl Authority for Auth {
    fn allows(&self, _: Capability) -> bool {
        self.0.get()
    }
}
#[derive(Default)]
struct Lane {
    active: Option<u16>,
    next_stream: u16,
    next_message: u16,
    repeat_stream: bool,
    repeat_message: bool,
    in_flight: Option<u16>,
}
impl Lane {
    fn new() -> Self {
        Self {
            active: None,
            next_stream: 11,
            next_message: 101,
            repeat_stream: false,
            repeat_message: false,
            in_flight: None,
        }
    }
}
impl DataLane for Lane {
    fn available(&self) -> bool {
        self.active.is_none()
    }
    fn acquire(&mut self) -> Result<u16, Error> {
        if self.active.is_some() {
            return Err(Error::Busy);
        }
        let id = self.next_stream;
        if !self.repeat_stream {
            self.next_stream += 1;
        }
        self.active = Some(id);
        Ok(id)
    }
    fn owns(&self, stream: u16) -> bool {
        self.active == Some(stream)
    }
    fn reserve_message(&mut self, stream: u16) -> Result<u16, Error> {
        if !self.owns(stream) {
            return Err(Error::Identity);
        }
        if self.in_flight.is_some() {
            return Err(Error::Busy);
        }
        let id = self.next_message;
        self.in_flight = Some(id);
        if !self.repeat_message {
            self.next_message += 1;
        }
        Ok(id)
    }
    fn release(&mut self, stream: u16) -> Result<(), Error> {
        if !self.owns(stream) {
            return Err(Error::Identity);
        }
        if self.in_flight.is_some() {
            return Err(Error::Busy);
        }
        self.active = None;
        Ok(())
    }
    fn finish_message(&mut self, stream: u16, message: u16) -> Result<(), Error> {
        if !self.owns(stream) || self.in_flight != Some(message) {
            return Err(Error::Identity);
        }
        self.in_flight = None;
        Ok(())
    }
}
struct Port<'a> {
    polls: u8,
    stop_calls: &'a Cell<u32>,
    invalid: bool,
    bad_loss: bool,
}
impl RawPort for Port<'_> {
    fn source_count(&self) -> usize {
        1
    }
    fn source(&self, index: usize) -> Option<Source> {
        (index == 0).then_some(Source {
            id: 7,
            kind: 1,
            channels: 3,
            formats: 1,
            flags: 1,
        })
    }
    fn channel(&self, source: u16, index: u8) -> Result<Channel, Error> {
        if source != 7 || index >= 3 {
            return Err(Error::Unsupported);
        }
        Ok(Channel {
            index,
            format: Format::I16,
            unit: 1,
            scale_num: 1,
            scale_den: 1000,
            axis: index + 1,
        })
    }
    fn configure(&mut self, c: Config) -> Result<Accepted, Error> {
        if c.rate_mhz != 100_000 {
            return Err(Error::Unsupported);
        }
        Ok(Accepted {
            rate_mhz: 100_000,
            period_ns: 10_000_000,
            stride: 6,
        })
    }
    fn start(&mut self, _: u16) -> Result<u64, Error> {
        Ok(1000)
    }
    fn poll(&mut self, out: &mut [u8], _: u16, _: u32) -> Result<Option<Polled>, Error> {
        self.polls += 1;
        out[..6].copy_from_slice(&[self.polls, 0, 2, 0, 3, 0]);
        Ok(Some(Polled {
            samples: 1,
            bytes: 6,
            lost: if self.bad_loss {
                u32::MAX
            } else if self.polls == 3 {
                2
            } else {
                0
            },
            t0_mono_us: 1000 + u64::from(self.polls) * 10000,
            quality_flags: 0,
            valid: !self.invalid,
        }))
    }
    fn stop(&mut self) -> Result<(), Error> {
        self.stop_calls.set(self.stop_calls.get() + 1);
        Ok(())
    }
}
fn config() -> Config {
    Config {
        source: 7,
        channel_mask: 7,
        rate_mhz: 100_000,
        range_code: 0,
        options: 0,
    }
}
fn port(c: &Cell<u32>) -> Port<'_> {
    Port {
        polls: 0,
        stop_calls: c,
        invalid: false,
        bad_loss: false,
    }
}
#[test]
fn real_catalog_configuration_and_external_lane_ids_are_used() {
    let calls = Cell::new(0);
    let mut p = port(&calls);
    let mut lane = Lane::new();
    let auth = Auth(Cell::new(true));
    let mut e =
        RawEngine::<82>::new(Profile::Development, Some(&mut p), &mut lane, &auth, 128).unwrap();
    let mut sources = [Source {
        id: 0,
        kind: 0,
        channels: 0,
        formats: 0,
        flags: 0,
    }; 4];
    assert_eq!(e.list(0, 4, &mut sources), Ok((255, 1)));
    assert_eq!(sources[0].id, 7);
    assert_eq!(e.channel(7, 2).unwrap().axis, 3);
    let cfg = e.configure(config()).unwrap();
    assert_eq!(cfg.accepted.stride, 6);
    let start = e.start(7, cfg.revision).unwrap();
    assert_eq!(start.stream_id, 11);
    assert_eq!(start.t0_mono_us, 1000);
    assert_eq!(start.max_batch_samples, 13);
    e.poll().unwrap();
    assert_eq!(e.begin_batch().err(), Some(Error::NoCredit));
    e.grant(11, 1).unwrap();
    let batch = e.begin_batch().unwrap();
    assert_eq!(batch.message_id, 101);
    assert_eq!(batch.samples, &[1, 0, 2, 0, 3, 0]);
    assert_eq!(e.current_batch().unwrap().message_id, 101);
    e.finish_batch(101).unwrap();
    assert_eq!(
        e.status(11).unwrap().counts,
        Counts {
            produced: 1,
            emitted: 1,
            dropped: 0,
            queued: 0
        }
    );
}
#[test]
fn queued_overflow_and_hardware_loss_preserve_sample_conservation() {
    let calls = Cell::new(0);
    let mut p = port(&calls);
    let mut lane = Lane::new();
    let auth = Auth(Cell::new(true));
    let mut e =
        RawEngine::<82>::new(Profile::Factory, Some(&mut p), &mut lane, &auth, 128).unwrap();
    let cfg = e.configure(config()).unwrap();
    e.start(7, cfg.revision).unwrap();
    for _ in 0..3 {
        e.poll().unwrap();
        let c = e.status(11).unwrap().counts;
        assert_eq!(c.produced, c.emitted + c.dropped + c.queued);
    }
    assert_eq!(
        e.status(11).unwrap().counts,
        Counts {
            produced: 5,
            emitted: 0,
            dropped: 4,
            queued: 1
        }
    );
    let final_ = e.stop(11).unwrap().unwrap();
    assert_eq!(
        final_,
        StopSummary {
            produced: 5,
            emitted: 0,
            dropped: 5,
            last_data_message_id: 0
        }
    );
    assert_eq!(e.stop(11), Ok(Some(final_)));
    assert_eq!(calls.get(), 1);
}
#[test]
fn stop_keeps_started_message_and_drops_unstarted_batch_until_drain() {
    let calls = Cell::new(0);
    let mut p = port(&calls);
    let mut lane = Lane::new();
    let auth = Auth(Cell::new(true));
    let mut e =
        RawEngine::<82>::new(Profile::Development, Some(&mut p), &mut lane, &auth, 128).unwrap();
    let cfg = e.configure(config()).unwrap();
    e.start(7, cfg.revision).unwrap();
    e.poll().unwrap();
    e.grant(11, 1).unwrap();
    let id = e.begin_batch().unwrap().message_id;
    e.poll().unwrap();
    assert_eq!(e.stop(11), Ok(None));
    assert_eq!(e.status(11).unwrap().state, State::Stopping);
    assert_eq!(e.start(7, cfg.revision), Err(Error::Busy));
    assert_eq!(e.begin_batch().err(), Some(Error::Busy));
    assert_eq!(e.current_batch().unwrap().samples, &[1, 0, 2, 0, 3, 0]);
    assert_eq!(e.finish_batch(id + 1), Err(Error::Identity));
    e.finish_batch(id).unwrap();
    assert_eq!(
        e.stop(11),
        Ok(Some(StopSummary {
            produced: 2,
            emitted: 1,
            dropped: 1,
            last_data_message_id: 101
        }))
    );
    assert_eq!(e.status(11).unwrap().state, State::Stopped);
}
#[test]
fn unsupported_config_invalid_samples_and_revocation_never_export_fake_data() {
    let calls = Cell::new(0);
    let mut p = port(&calls);
    p.invalid = true;
    let mut lane = Lane::new();
    let auth = Auth(Cell::new(true));
    let mut e =
        RawEngine::<82>::new(Profile::Factory, Some(&mut p), &mut lane, &auth, 128).unwrap();
    assert_eq!(
        e.configure(Config {
            range_code: 1,
            ..config()
        }),
        Err(Error::Invalid)
    );
    assert_eq!(
        e.configure(Config {
            channel_mask: 8,
            ..config()
        }),
        Err(Error::Invalid)
    );
    assert_eq!(
        e.configure(Config {
            rate_mhz: 200_000,
            ..config()
        }),
        Err(Error::Unsupported)
    );
    let cfg = e.configure(config()).unwrap();
    e.start(7, cfg.revision).unwrap();
    e.poll().unwrap();
    assert_eq!(
        e.status(11).unwrap().counts,
        Counts {
            produced: 1,
            emitted: 0,
            dropped: 1,
            queued: 0
        }
    );
    e.grant(11, 1).unwrap();
    assert_eq!(e.begin_batch().err(), Some(Error::NotFound));
    auth.0.set(false);
    assert_eq!(e.status(11), Err(Error::Denied));
    auth.0.set(true);
    assert_eq!(e.status(11), Err(Error::Closed));
    assert_eq!(calls.get(), 1);
}
#[test]
fn commercial_missing_backend_and_occupied_lane_are_rejected() {
    let calls = Cell::new(0);
    let mut p = port(&calls);
    let mut lane = Lane::new();
    let auth = Auth(Cell::new(true));
    assert_eq!(
        RawEngine::<82>::new(Profile::Commercial, Some(&mut p), &mut lane, &auth, 128).err(),
        Some(Error::Commercial)
    );
    assert_eq!(
        RawEngine::<82>::new(Profile::Development, None, &mut lane, &auth, 128).err(),
        Some(Error::Unsupported)
    );
    lane.active = Some(99);
    assert_eq!(
        RawEngine::<82>::new(Profile::Development, Some(&mut p), &mut lane, &auth, 128).err(),
        Some(Error::Busy)
    );
}
#[test]
fn cumulative_grant_is_idempotent_and_pending_payload_debug_is_redacted() {
    let calls = Cell::new(0);
    let mut p = port(&calls);
    let mut lane = Lane::new();
    let auth = Auth(Cell::new(true));
    let mut e =
        RawEngine::<82>::new(Profile::Development, Some(&mut p), &mut lane, &auth, 128).unwrap();
    let cfg = e.configure(config()).unwrap();
    e.start(7, cfg.revision).unwrap();
    e.grant(11, 1).unwrap();
    e.grant(11, 1).unwrap();
    assert_eq!(e.grant(11, 0), Err(Error::Conflict));
    e.poll().unwrap();
    e.begin_batch().unwrap();
    assert!(!format!("{e:?}").contains("[1, 0, 2"));
    assert!(!format!("{:?}", e.current_batch().unwrap()).contains("[1, 0, 2"));
    e.finish_batch(101).unwrap();
    e.poll().unwrap();
    assert_eq!(e.begin_batch().err(), Some(Error::NoCredit));
}

#[test]
fn unfinished_stop_has_a_ten_second_deadline_and_closes_the_connection() {
    let calls = Cell::new(0);
    let mut p = port(&calls);
    let mut lane = Lane::new();
    let auth = Auth(Cell::new(true));
    let mut e =
        RawEngine::<82>::new(Profile::Development, Some(&mut p), &mut lane, &auth, 128).unwrap();
    let cfg = e.configure(config()).unwrap();
    e.start(7, cfg.revision).unwrap();
    e.advance(100).unwrap();
    e.poll().unwrap();
    e.grant(11, 1).unwrap();
    e.begin_batch().unwrap();
    assert_eq!(e.stop(11), Ok(None));
    assert_eq!(e.advance(10099), Ok(()));
    assert_eq!(e.advance(10100), Err(Error::Timeout));
    assert_eq!(e.current_batch().err(), Some(Error::Closed));
}

#[test]
fn reused_external_ids_are_rejected_across_capture_replacement() {
    for repeat_stream in [false, true] {
        let calls = Cell::new(0);
        let mut p = port(&calls);
        let mut lane = Lane::new();
        lane.repeat_stream = repeat_stream;
        lane.repeat_message = !repeat_stream;
        let auth = Auth(Cell::new(true));
        let mut e = RawEngine::<82>::new(Profile::Development, Some(&mut p), &mut lane, &auth, 128)
            .unwrap();
        let cfg = e.configure(config()).unwrap();
        e.start(7, cfg.revision).unwrap();
        e.poll().unwrap();
        e.grant(11, 1).unwrap();
        e.begin_batch().unwrap();
        e.finish_batch(101).unwrap();
        e.stop(11).unwrap();
        if repeat_stream {
            assert_eq!(e.start(7, cfg.revision), Err(Error::Identity));
        } else {
            e.start(7, cfg.revision).unwrap();
            e.poll().unwrap();
            e.grant(12, 1).unwrap();
            assert_eq!(e.begin_batch().err(), Some(Error::Identity));
        }
    }
}

#[test]
fn shared_owner_observes_each_completed_batch_before_the_next_reservation() {
    let calls = Cell::new(0);
    let mut p = port(&calls);
    let mut lane = Lane::new();
    let auth = Auth(Cell::new(true));
    let mut e =
        RawEngine::<82>::new(Profile::Development, Some(&mut p), &mut lane, &auth, 128).unwrap();
    let cfg = e.configure(config()).unwrap();
    e.start(7, cfg.revision).unwrap();
    e.grant(11, 2).unwrap();
    e.poll().unwrap();
    e.begin_batch().unwrap();
    assert_eq!(e.finish_batch(100), Err(Error::Identity));
    e.finish_batch(101).unwrap();
    e.poll().unwrap();
    assert_eq!(e.begin_batch().unwrap().message_id, 102);
    e.finish_batch(102).unwrap();
    assert_eq!(e.stop(11).unwrap().unwrap().emitted, 2);
}

#[test]
fn malformed_backend_count_cannot_wrap_or_report_fabricated_final_statistics() {
    let calls = Cell::new(0);
    let mut p = port(&calls);
    p.bad_loss = true;
    let mut lane = Lane::new();
    let auth = Auth(Cell::new(true));
    let mut e =
        RawEngine::<82>::new(Profile::Development, Some(&mut p), &mut lane, &auth, 128).unwrap();
    let cfg = e.configure(config()).unwrap();
    e.start(7, cfg.revision).unwrap();
    assert_eq!(e.poll(), Err(Error::Capacity));
    assert_eq!(calls.get(), 1);
    assert_eq!(e.status(11), Err(Error::Backend));
    assert_eq!(e.stop(11), Err(Error::Backend));
}
