//! Build profile rejection coverage.
#[path = "../profile.rs"]
mod profile;
const PROFILE: &str = include_str!("../profiles/legacy-lab-test.profile");
#[test]
fn test_should_require_exact_explicit_test_fields() {
    assert!(profile::parse(PROFILE).is_ok());
    for (a, b) in [
        ("kind=wattch5-bringup-test-v1", "kind=product"),
        ("name=W5-TEST", "name="),
        ("name=W5-TEST", "name=very-long-twenty-one-character-name"),
        ("address=4853545200c0", "address=485354520000"),
        ("rx_uuid=efe1", "rx_uuid=efe0"),
        ("service_uuid=efe0", "service_uuid=1800"),
        ("product_id=0", "product_id=33939"),
    ] {
        assert!(profile::parse(&PROFILE.replace(a, b)).is_err(), "{a}");
    }
    assert!(profile::parse(&format!("{PROFILE}name=duplicate\n")).is_err());
    assert!(profile::parse(&PROFILE.replace("name=W5-TEST", "other=W5-TEST")).is_err());
}
