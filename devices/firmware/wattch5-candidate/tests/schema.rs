//! New candidate namespaces retain complete existing typed bodies; no legacy slot conversion.
use device_history_store::HistorySchema;
use device_storage::StoredRecord;
use wattch5_candidate::schema::StrictSchema;
#[test]
fn test_should_reject_cross_domain_unknown_schema_and_parent_substitution() {
    let bytes = [1, 0, 0, 0, 1, 80, 0];
    let record = StoredRecord {
        domain: 1,
        epoch: 1,
        record_id: 1,
        record_type: 0x0021,
        schema: 1,
        utc_ms: 0,
        flags: 0,
        payload: &bytes,
    };
    assert!(StrictSchema.validate(record, 0).is_ok());
    assert!(StrictSchema.validate(record, 7).is_err());
    for domain in [0, 2, 5, 6, 7] {
        assert!(
            StrictSchema
                .validate(StoredRecord { domain, ..record }, 0)
                .is_err()
        );
    }
    assert!(
        StrictSchema
            .validate(
                StoredRecord {
                    schema: 9,
                    ..record
                },
                0
            )
            .is_err()
    );
    assert!(
        StrictSchema
            .validate(
                StoredRecord {
                    payload: &[1; 474],
                    ..record
                },
                0
            )
            .is_err()
    );
}
