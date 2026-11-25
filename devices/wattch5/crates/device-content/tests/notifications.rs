//! Source-phone authorization, lifecycle cleanup and call result semantics.
#![cfg(feature = "notifications")]
use device_content::*;
#[derive(Debug)]
struct Access;
impl AccessPort for Access {
    fn authorize(&self, _: Scope, _: Operation) -> Result<(), Error> {
        Ok(())
    }
}
#[derive(Debug)]
struct Deny;
impl AccessPort for Deny {
    fn authorize(&self, _: Scope, _: Operation) -> Result<(), Error> {
        Err(Error::Denied)
    }
}
#[derive(Debug, Default)]
struct Port {
    alerts: u32,
    requests: u32,
    withdrawals: u32,
    missing: bool,
}
impl NotificationPort for Port {
    fn supported_actions(&self, _: Scope) -> Result<u16, Error> {
        if self.missing {
            Err(Error::Unavailable)
        } else {
            Ok(7)
        }
    }
    fn present(&mut self, _: Scope, _: u32, _: Notification<'_>, alert: bool) -> Result<(), Error> {
        if alert {
            self.alerts += 1;
        }
        Ok(())
    }
    fn withdraw(&mut self, _: Scope, _: u32) -> Result<(), Error> {
        self.withdrawals += 1;
        Ok(())
    }
    fn request_action(&mut self, _: Scope, _: u32, _: u8) -> Result<(), Error> {
        self.requests += 1;
        Ok(())
    }
}
fn call() -> Notification<'static> {
    Notification {
        notification: 42,
        category: 1,
        source_app: "phone",
        sender: "A",
        number: "123",
        title: "call",
        body: "",
        action_mask: 7,
        created_utc_ms: 0,
        created_time_valid: false,
    }
}
#[test]
fn test_should_publish_once_only_after_admission_and_not_alert_on_update() -> Result<(), Error> {
    let mut service = NotificationService::<2, 4>::new(Scope::new(1, 1)?);
    let mut port = Port::default();
    assert_eq!(
        service.register(1, call(), &Access, &port)?,
        NotificationChange::New
    );
    assert_eq!(port.alerts, 0);
    service.content_response_admitted(1, &Access, &mut port)?;
    assert_eq!(
        service.register(1, call(), &Access, &port)?,
        NotificationChange::Unchanged
    );
    service.content_response_admitted(1, &Access, &mut port)?;
    assert_eq!(port.alerts, 1);
    let mut updated = call();
    updated.body = "updated";
    assert_eq!(
        service.register(1, updated, &Access, &port)?,
        NotificationChange::Updated
    );
    service.content_response_admitted(1, &Access, &mut port)?;
    assert_eq!(port.alerts, 1);
    assert_eq!(
        service.register(2, call(), &Access, &port),
        Err(Error::Conflict)
    );
    Ok(())
}
#[test]
fn test_should_settle_all_notification_results_without_replaying_action() -> Result<(), Error> {
    for result in 1..=5 {
        let mut service = NotificationService::<2, 4>::new(Scope::new(1, 1)?);
        let mut port = Port::default();
        service.register(1, call(), &Access, &port)?;
        service.content_response_admitted(1, &Access, &mut port)?;
        service.prepare_action(42, 1, &Access, &port)?;
        assert_eq!(service.settle(42, 1, result, &Access), Err(Error::State));
        service.action_response_admitted(42, 1, &Access, &mut port)?;
        service.settle(42, 1, result, &Access)?;
        service.settle(42, 1, result, &Access)?;
        assert_eq!(
            service.settle(42, 1, if result == 1 { 2 } else { 1 }, &Access),
            Err(Error::Conflict)
        );
        assert_eq!(service.settle(42, 1, 0, &Access), Err(Error::Invalid));
        service.action_response_admitted(42, 1, &Access, &mut port)?;
        assert_eq!(port.requests, 1);
        assert_eq!(service.action_event(42, 1, &mut [0; 8]), Err(Error::State));
    }
    Ok(())
}
#[test]
fn test_should_clear_pending_and_result_ledger_on_delete_and_owner_change() -> Result<(), Error> {
    let mut service = NotificationService::<2, 4>::new(Scope::new(1, 1)?);
    let mut port = Port::default();
    service.register(1, call(), &Access, &port)?;
    service.content_response_admitted(1, &Access, &mut port)?;
    service.prepare_action(42, 1, &Access, &port)?;
    service.action_response_admitted(42, 1, &Access, &mut port)?;
    service.settle(42, 1, 1, &Access)?;
    service.prepare_action(42, 2, &Access, &port)?;
    service.delete_response_admitted(1, &Access, &mut port)?;
    assert_eq!(port.withdrawals, 1);
    assert_eq!(service.settle(42, 1, 1, &Access), Err(Error::NotFound));
    assert_eq!(service.action_state(42, 2), Err(Error::NotFound));
    service.register(1, call(), &Access, &port)?;
    service.content_response_admitted(1, &Access, &mut port)?;
    service.prepare_action(42, 1, &Access, &port)?;
    assert_eq!(service.replace_scope(Scope::new(2, 2)?)?.len(), 1);
    assert_eq!(service.settle(42, 1, 1, &Access), Err(Error::NotFound));
    Ok(())
}
#[test]
fn test_should_clear_pending_on_cccd_loss_and_require_trusted_source() -> Result<(), Error> {
    let mut service = NotificationService::<2, 4>::new(Scope::new(1, 1)?);
    let mut port = Port::default();
    assert_eq!(
        service.register(1, call(), &Deny, &port),
        Err(Error::Denied)
    );
    port.missing = true;
    assert_eq!(
        service.register(1, call(), &Access, &port),
        Err(Error::Unavailable)
    );
    port.missing = false;
    service.register(1, call(), &Access, &port)?;
    service.content_response_admitted(1, &Access, &mut port)?;
    service.prepare_action(42, 1, &Access, &port)?;
    service.action_response_admitted(42, 1, &Access, &mut port)?;
    assert_eq!(service.settle(42, 1, 1, &Deny), Err(Error::Denied));
    let unknown = service.invalidate();
    assert_eq!(unknown.len(), 1);
    assert_eq!(
        unknown.outcome(0)?,
        (42, 1, NotificationActionState::Unknown)
    );
    assert_eq!(service.settle(42, 1, 1, &Access), Err(Error::NotFound));
    assert_eq!(
        service.prepare_action(42, 1, &Access, &port),
        Err(Error::NotFound)
    );
    Ok(())
}
#[test]
fn test_should_reject_noncall_actions_and_missing_action_bits() -> Result<(), Error> {
    let mut service = NotificationService::<2, 4>::new(Scope::new(1, 1)?);
    let mut port = Port::default();
    let mut notification = call();
    notification.category = 2;
    notification.action_mask = 0;
    service.register(1, notification, &Access, &port)?;
    service.content_response_admitted(1, &Access, &mut port)?;
    assert_eq!(
        service.prepare_action(42, 1, &Access, &port),
        Err(Error::Denied)
    );
    notification = call();
    notification.action_mask = 2;
    service.register(1, notification, &Access, &port)?;
    service.content_response_admitted(1, &Access, &mut port)?;
    assert_eq!(
        service.prepare_action(42, 1, &Access, &port),
        Err(Error::Denied)
    );
    assert_eq!(
        service.prepare_action(42, 0, &Access, &port),
        Err(Error::Invalid)
    );
    Ok(())
}
#[test]
fn test_should_recheck_call_permission_before_admitting_prepared_action() -> Result<(), Error> {
    let mut service = NotificationService::<2, 4>::new(Scope::new(1, 1)?);
    let mut port = Port::default();
    service.register(1, call(), &Access, &port)?;
    service.content_response_admitted(1, &Access, &mut port)?;
    service.prepare_action(42, 1, &Access, &port)?;
    let mut ended = call();
    ended.category = 2;
    ended.action_mask = 0;
    service.register(1, ended, &Access, &port)?;
    service.content_response_admitted(1, &Access, &mut port)?;
    assert_eq!(
        service.action_response_admitted(42, 1, &Access, &mut port),
        Err(Error::Denied)
    );
    assert_eq!(port.requests, 0);
    Ok(())
}
