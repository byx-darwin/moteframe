use wattch5_product::{HELLO_VECTOR, SelfCheckError, self_check, service_self_check};

#[test]
fn checks_real_codecs_fragmentation_and_standard_crc_vector() -> Result<(), SelfCheckError> {
    let result = self_check(&HELLO_VECTOR)?;
    assert_eq!(result.checks_passed, 4);
    assert_eq!(result.fragments, 3);
    assert_eq!(result.crc32c_check, 0xe306_9283);
    Ok(())
}

#[test]
fn rejects_corrupted_protocol_vector_instead_of_reporting_success() {
    let mut corrupt = HELLO_VECTOR;
    corrupt[3] = 1;
    assert!(self_check(&corrupt).is_err());
    assert!(self_check(&HELLO_VECTOR[..9]).is_err());
}

#[test]
fn exercises_core_dispatch_with_restricted_access_and_closed_lifecycle()
-> Result<(), SelfCheckError> {
    assert_eq!(service_self_check()?, 6);
    Ok(())
}
