//! Exact existing physiological/motion bodies in the source-selected six domains.
use device_history_service::HistoryFailure as Failure;
use device_history_store::HistorySchema;
use device_storage::StoredRecord;
/// Strict typed validator. No synthetic records, provenance or producer is installed.
#[derive(Debug)]
pub struct StrictSchema;
impl HistorySchema for StrictSchema {
    fn validate(&self, r: StoredRecord<'_>, parent: u64) -> Result<(), Failure> {
        let invalid = |_| Failure::StoredIntegrity;
        macro_rules! sample {
            ($ty:ty) => {{
                let v = <$ty>::decode(r.payload).map_err(invalid)?;
                if parent == 0 || v.session != parent {
                    return Err(Failure::StoredIntegrity);
                }
                Ok(())
            }};
        }
        match (r.domain, r.record_type, r.schema) {
            (0 | 5, 0x2301, 1) if parent == 0 => {
                let value = device_motion::ActivityRecord::decode(r.payload).map_err(invalid)?;
                if (r.domain == 0 && value.kind != 1)
                    || (r.domain == 5 && !matches!(value.kind, 2 | 3))
                {
                    return Err(Failure::StoredIntegrity);
                }
                Ok(())
            }
            (1, 0x0021, 1) if parent == 0 => device_motion::SleepInterval::decode(r.payload)
                .map(|_| ())
                .map_err(invalid),
            (6, 0x2201, 2) if parent == 0 => device_motion::SportSummary::decode(r.payload)
                .map(|_| ())
                .map_err(invalid),
            (6, 0x2202, 1) => sample!(device_motion::SportHeartSample),
            (6, 0x2203, 1) => sample!(device_motion::SportCadenceSample),
            (6, 0x2204, 1) => sample!(device_motion::SportRriSample),
            (6, 0x2205, 1) => sample!(device_motion::SportRespirationSample),
            (6, 0x2206, 1) => sample!(device_motion::SportPaceSample),
            (6, 0x2207, 1) => sample!(device_motion::SportDistanceSample),
            (6, 0x2208, 2) => sample!(device_motion::SportGpsSample),
            (2, 0x0020, 1) if parent == 0 => device_health::HeartRate::decode(r.payload)
                .map(|_| ())
                .map_err(|_| Failure::StoredIntegrity),
            (7, 0x2801, 1) if parent == 0 => device_health::Temperature::decode(r.payload)
                .map(|_| ())
                .map_err(|_| Failure::StoredIntegrity),
            _ => Err(Failure::StoredIntegrity),
        }
    }
}
