#[test]
fn test_should_select_only_explicit_protocol_features() {
    for (module, selected) in [
        (2, cfg!(feature = "device-jobs")),
        (0x70, cfg!(feature = "raw-data")),
        (0x71, cfg!(feature = "self-test")),
        (0x72, cfg!(feature = "engineering")),
        (0x73, cfg!(feature = "factory-config")),
    ] {
        assert_eq!(
            device_maintenance_service::module_compiled(module),
            selected
        );
    }
    assert!(!device_maintenance_service::module_compiled(0x74));
}
#[test]
fn test_should_reject_trailing_and_zero_operation_bindings() {
    use device_maintenance_service::NamedRequest;
    assert!(NamedRequest::decode(&[0; 22]).is_err());
    let mut body = [1; 22];
    body[16..].copy_from_slice(&[1, 0, 1, 0, 1, 0]);
    assert!(NamedRequest::decode(&body).is_err());
    body[18..20].copy_from_slice(&[2, 0]);
    let decoded = NamedRequest::decode(&body).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(decoded.params, &[1, 0]);
    assert!(!format!("{decoded:?}").contains("operation_id"));
}
