use wattch5_product::{HELLO_VECTOR, self_check, service_self_check};

fn main() -> Result<(), String> {
    let report = self_check(&HELLO_VECTOR).map_err(|error| format!("{error:?}"))?;
    let service_checks = service_self_check().map_err(|error| format!("{error:?}"))?;
    #[cfg(feature = "wattch5-source")]
    let source_checks =
        wattch5_product::product::source_self_check().map_err(|error| format!("{error:?}"))?;
    #[cfg(not(feature = "wattch5-source"))]
    let source_checks = 0;
    println!(
        "{{\"execution_target\":\"host\",\"checks_passed\":{},\"fragments\":{},\"crc32c_check\":\"{:08x}\",\"service_checks_passed\":{},\"source_catalogue_checks\":{}}}",
        report.checks_passed, report.fragments, report.crc32c_check, service_checks, source_checks,
    );
    Ok(())
}
