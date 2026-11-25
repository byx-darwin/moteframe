//! Pure heartbeat timing and frozen token echo behavior.
#![cfg(feature = "phone")]
use device_content::*;
#[derive(Debug)]
struct Access;
impl AccessPort for Access {
    fn authorize(&self, _: Scope, _: Operation) -> Result<(), Error> {
        Ok(())
    }
}
fn manifest() -> Manifest<'static> {
    Manifest {
        heartbeat_timeout_s: Some(Range { min: 1, max: 120 }),
        heartbeat_grace_s: Some(Range { min: 0, max: 20 }),
        ..Manifest::default()
    }
}
#[test]
fn test_should_accept_only_explicit_heartbeat_policy_ranges() -> Result<(), Error> {
    let mut monitor = HeartbeatMonitor::new(Scope::new(1, 1)?);
    assert_eq!(
        monitor.configure(
            HeartbeatPolicy {
                enabled: true,
                timeout_s: 10,
                grace_s: 2
            },
            &manifest(),
            100,
            &Access
        ),
        Ok(())
    );
    Ok(())
}
#[test]
fn test_should_use_external_clock_without_claiming_unreceived_heartbeat() -> Result<(), Error> {
    let mut monitor = HeartbeatMonitor::new(Scope::new(1, 1)?);
    monitor.configure(
        HeartbeatPolicy {
            enabled: true,
            timeout_s: 10,
            grace_s: 2,
        },
        &manifest(),
        100,
        &Access,
    )?;
    assert_eq!(monitor.poll(100)?, HeartbeatState::Waiting);
    let mut output = [0; 4];
    assert_eq!(
        monitor.heartbeat_bytes(&[0x78, 0x56, 0x34, 0x12], &mut output, 200, &Access)?,
        4
    );
    assert_eq!(output, [0x78, 0x56, 0x34, 0x12]);
    assert_eq!(monitor.poll(10_199)?, HeartbeatState::Healthy);
    assert_eq!(monitor.poll(10_200)?, HeartbeatState::Grace);
    assert_eq!(monitor.poll(12_200)?, HeartbeatState::TimedOut);
    assert_eq!(monitor.poll(12_199), Err(Error::State));
    monitor.invalidate();
    assert_eq!(monitor.poll(12_200)?, HeartbeatState::Disabled);
    Ok(())
}
#[test]
fn test_should_not_refresh_on_truncated_echo_buffer() -> Result<(), Error> {
    let mut monitor = HeartbeatMonitor::new(Scope::new(1, 1)?);
    monitor.configure(
        HeartbeatPolicy {
            enabled: true,
            timeout_s: 10,
            grace_s: 2,
        },
        &manifest(),
        0,
        &Access,
    )?;
    assert_eq!(
        monitor.heartbeat_bytes(&[1, 0, 0, 0], &mut [0; 3], 9999, &Access),
        Err(Error::Capacity)
    );
    assert_eq!(monitor.poll(10_000)?, HeartbeatState::Grace);
    assert_eq!(
        monitor.heartbeat_bytes(&[1, 0, 0], &mut [0; 4], 10_000, &Access),
        Err(Error::Invalid)
    );
    Ok(())
}
