//! Feature selection keeps stable module IDs and separates wire, module and value revisions.
use device_application::COMPILED_MODULES;
#[test]
fn test_should_keep_a_sorted_unique_catalogue_and_frozen_core_family_versions() {
    assert_eq!(
        COMPILED_MODULES
            .first()
            .map(|m| (m.module, m.major, m.minor)),
        Some((0, 1, 2))
    );
    assert!(
        COMPILED_MODULES
            .windows(2)
            .all(|p| p[0].module < p[1].module)
    );
    for m in COMPILED_MODULES {
        let expected = match m.module {
            0 | 0x10 | 0x70 => (1, 2),
            0x71 => (1, 1),
            _ => (1, 0),
        };
        assert_eq!((m.major, m.minor), expected);
    }
}
#[cfg(feature = "system")]
#[test]
fn test_should_include_device_contract_when_only_system_actions_are_selected() {
    assert!(COMPILED_MODULES.iter().any(|m| m.module == 2));
}
#[cfg(feature = "device-jobs")]
#[test]
fn test_should_include_device_contract_when_only_commercial_persistent_jobs_are_selected() {
    assert!(COMPILED_MODULES.iter().any(|m| m.module == 2));
}
