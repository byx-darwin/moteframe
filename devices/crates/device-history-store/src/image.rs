use crate::{Data, HistorySchema, Record};
use device_history_service::HistoryFailure;
use device_protocol::HistoryDomain;
use device_storage::CommitState;
const MAGIC: &[u8; 8] = b"DHIST001";

struct Writer<'a> {
    out: &'a mut [u8],
    at: usize,
}
impl Writer<'_> {
    fn put(&mut self, bytes: &[u8]) -> Result<(), HistoryFailure> {
        let end = self
            .at
            .checked_add(bytes.len())
            .ok_or(HistoryFailure::Capacity)?;
        self.out
            .get_mut(self.at..end)
            .ok_or(HistoryFailure::Capacity)?
            .copy_from_slice(bytes);
        self.at = end;
        Ok(())
    }
}
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}
impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], HistoryFailure> {
        let end = self
            .at
            .checked_add(n)
            .ok_or(HistoryFailure::StoredIntegrity)?;
        let bytes = self
            .bytes
            .get(self.at..end)
            .ok_or(HistoryFailure::StoredIntegrity)?;
        self.at = end;
        Ok(bytes)
    }
    fn number<const N: usize>(&mut self) -> Result<[u8; N], HistoryFailure> {
        self.take(N)?
            .try_into()
            .map_err(|_| HistoryFailure::StoredIntegrity)
    }
}
pub(crate) fn encode<const D: usize, const R: usize, const B: usize>(
    data: &Data<D, R, B>,
    out: &mut [u8],
) -> Result<usize, HistoryFailure> {
    let mut w = Writer { out, at: 0 };
    w.put(MAGIC)?;
    w.put(&data.dataset)?;
    w.put(
        &u16::try_from(data.domains_len)
            .map_err(|_| HistoryFailure::Capacity)?
            .to_le_bytes(),
    )?;
    w.put(
        &u32::try_from(data.records_len)
            .map_err(|_| HistoryFailure::Capacity)?
            .to_le_bytes(),
    )?;
    for (i, d) in data.domains[..data.domains_len].iter().enumerate() {
        let mut bytes = [0; 21];
        d.encode(&mut bytes)
            .map_err(|_| HistoryFailure::StoredIntegrity)?;
        w.put(&bytes)?;
        w.put(&data.high[i].to_le_bytes())?;
        w.put(&data.ack[i].to_le_bytes())?;
    }
    for r in &data.records[..data.records_len] {
        w.put(&[r.domain])?;
        w.put(&r.epoch.to_le_bytes())?;
        w.put(&r.id.to_le_bytes())?;
        w.put(&r.kind.to_le_bytes())?;
        w.put(&r.schema.to_le_bytes())?;
        w.put(&r.utc.to_le_bytes())?;
        w.put(&r.flags.to_le_bytes())?;
        w.put(&r.parent.to_le_bytes())?;
        w.put(
            &u16::try_from(r.len)
                .map_err(|_| HistoryFailure::Capacity)?
                .to_le_bytes(),
        )?;
        w.put(&r.crc.to_le_bytes())?;
        w.put(&r.body[..r.len])?;
    }
    Ok(w.at)
}
pub(crate) fn decode<const D: usize, const R: usize, const B: usize>(
    bytes: &[u8],
    schema: &impl HistorySchema,
) -> Result<Data<D, R, B>, HistoryFailure> {
    let mut r = Reader { bytes, at: 0 };
    if r.take(8)? != MAGIC {
        return Err(HistoryFailure::StoredIntegrity);
    }
    let mut data = Data::EMPTY;
    data.dataset = r.number()?;
    data.domains_len = usize::from(u16::from_le_bytes(r.number()?));
    data.records_len = usize::try_from(u32::from_le_bytes(r.number()?))
        .map_err(|_| HistoryFailure::StoredIntegrity)?;
    if data.dataset == [0; 16]
        || data.domains_len == 0
        || data.domains_len > D
        || data.records_len > R
    {
        return Err(HistoryFailure::StoredIntegrity);
    }
    for i in 0..data.domains_len {
        data.domains[i] =
            HistoryDomain::decode(r.take(21)?).map_err(|_| HistoryFailure::StoredIntegrity)?;
        data.high[i] = u64::from_le_bytes(r.number()?);
        data.ack[i] = u64::from_le_bytes(r.number()?);
        if i > 0 && data.domains[i - 1].domain >= data.domains[i].domain
            || data.ack[i] > data.high[i]
            || data.domains[i].newest_id > data.high[i]
        {
            return Err(HistoryFailure::StoredIntegrity);
        }
    }
    for i in 0..data.records_len {
        let record = decode_record::<B>(&mut r)?;
        let d = data
            .domain(record.domain)
            .map_err(|_| HistoryFailure::StoredIntegrity)?;
        if record.epoch != data.domains[d].epoch
            || record.id > data.high[d]
            || i > 0
                && (data.records[i - 1].domain, data.records[i - 1].id)
                    >= (record.domain, record.id)
        {
            return Err(HistoryFailure::StoredIntegrity);
        }
        record
            .stored()
            .verify(CommitState::Committed, record.crc)
            .map_err(|_| HistoryFailure::StoredIntegrity)?;
        schema
            .validate(record.stored(), record.parent)
            .map_err(|_| HistoryFailure::StoredIntegrity)?;
        data.records[i] = record;
    }
    if r.at != bytes.len() {
        return Err(HistoryFailure::StoredIntegrity);
    }
    for (index, d) in data.domains[..data.domains_len].iter().enumerate() {
        let mut matching = data.records[..data.records_len]
            .iter()
            .filter(|r| r.domain == d.domain);
        let first = matching.next();
        let last = matching.next_back().or(first);
        if d.oldest_id != first.map_or(0, |r| r.id)
            || d.newest_id != last.map_or(0, |r| r.id)
            || data.ack[index] != 0
                && !data.records[..data.records_len]
                    .iter()
                    .any(|r| r.domain == d.domain && r.id == data.ack[index])
        {
            return Err(HistoryFailure::StoredIntegrity);
        }
    }
    Ok(data)
}
fn decode_record<const B: usize>(r: &mut Reader<'_>) -> Result<Record<B>, HistoryFailure> {
    let mut record = Record::EMPTY;
    record.domain = r.take(1)?[0];
    record.epoch = u32::from_le_bytes(r.number()?);
    record.id = u64::from_le_bytes(r.number()?);
    record.kind = u16::from_le_bytes(r.number()?);
    record.schema = u16::from_le_bytes(r.number()?);
    record.utc = i64::from_le_bytes(r.number()?);
    record.flags = u16::from_le_bytes(r.number()?);
    record.parent = u64::from_le_bytes(r.number()?);
    record.len = usize::from(u16::from_le_bytes(r.number()?));
    record.crc = u32::from_le_bytes(r.number()?);
    if record.len > B {
        return Err(HistoryFailure::StoredIntegrity);
    }
    record.body[..record.len].copy_from_slice(r.take(record.len)?);
    Ok(record)
}
