use super::{HistoryFailure, LogConfig, Record, State, to_u16, to_u32};
use device_protocol::{Crc32c, HistoryDomain};
use device_storage::persistence::Geometry;
/// Pure byte-format context shared by synchronous and asynchronous owners.
pub(crate) struct Format<'a, const D: usize, const R: usize, const B: usize, const N: usize> {
    pub(crate) config: LogConfig,
    pub(crate) geometry: Geometry,
    pub(crate) stride: u32,
    pub(crate) data: &'a State<D>,
}
pub(crate) fn crc(parts: &[&[u8]]) -> u32 {
    let mut c = Crc32c::new();
    for b in parts {
        c.update(b);
    }
    c.finish()
}
pub(crate) fn u16at(b: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([b[i], b[i + 1]])
}
pub(crate) fn u32at(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}
pub(crate) fn u64at(b: &[u8], i: usize) -> u64 {
    u64::from_le_bytes([
        b[i],
        b[i + 1],
        b[i + 2],
        b[i + 3],
        b[i + 4],
        b[i + 5],
        b[i + 6],
        b[i + 7],
    ])
}
impl<const D: usize, const R: usize, const B: usize, const N: usize> Format<'_, D, R, B, N> {
    pub(crate) fn config_bytes(&self) -> Result<[u8; 48], HistoryFailure> {
        let mut b = [0; 48];
        b[..16].copy_from_slice(&self.config.namespace);
        b[16..20].copy_from_slice(&self.config.log_start.to_le_bytes());
        b[20..24].copy_from_slice(&self.geometry.capacity.to_le_bytes());
        b[24..28].copy_from_slice(&self.geometry.write_size.to_le_bytes());
        b[28..32].copy_from_slice(&self.geometry.erase_size.to_le_bytes());
        b[32..36].copy_from_slice(&self.stride.to_le_bytes());
        b[36..40].copy_from_slice(&self.config.slots_per_domain.to_le_bytes());
        b[40..42].copy_from_slice(&to_u16(B)?.to_le_bytes());
        b[42..44].copy_from_slice(&to_u16(R)?.to_le_bytes());
        b[44..46].copy_from_slice(&to_u16(self.data.len)?.to_le_bytes());
        Ok(b)
    }
    pub(crate) fn encode(
        &self,
        data: &State<D>,
        out: &mut [u8; N],
    ) -> Result<usize, HistoryFailure> {
        self.encode_slice(data, out)
    }
    pub(crate) fn encode_slice(
        &self,
        data: &State<D>,
        out: &mut [u8],
    ) -> Result<usize, HistoryFailure> {
        let len = 80 + data.len * 64;
        if len > N || len > out.len() {
            return Err(HistoryFailure::Capacity);
        }
        out[..len].fill(0);
        out[..8].copy_from_slice(b"HPGMET01");
        out[8..24].copy_from_slice(&data.dataset);
        out[24..72].copy_from_slice(&self.config_bytes()?);
        for d in 0..data.len {
            let b = &mut out[80 + d * 64..80 + (d + 1) * 64];
            b[0] = data.domains[d].domain;
            b[4..8].copy_from_slice(&data.domains[d].epoch.to_le_bytes());
            b[8..16].copy_from_slice(&data.domains[d].newest_id.to_le_bytes());
            b[16..24].copy_from_slice(&data.ack[d].to_le_bytes());
            b[24..28].copy_from_slice(&data.count[d].to_le_bytes());
            b[28..32].copy_from_slice(&data.tail[d].to_le_bytes());
            b[32..40].copy_from_slice(&data.domains[d].oldest_id.to_le_bytes());
            b[40..48].copy_from_slice(&data.domains[d].newest_id.to_le_bytes());
        }
        Ok(len)
    }
    pub(crate) fn decode(&self, b: &[u8]) -> Result<State<D>, HistoryFailure> {
        let mut state = State::EMPTY;
        self.decode_into(b, &mut state)?;
        Ok(state)
    }
    pub(crate) fn decode_into(&self, b: &[u8], s: &mut State<D>) -> Result<(), HistoryFailure> {
        if b.len() < 80 || &b[..8] != b"HPGMET01" || b[8..24] == [0; 16] || b[70..80] != [0; 10] {
            return Err(HistoryFailure::StoredIntegrity);
        }
        let len = u16at(b, 68) as usize;
        if len == 0 || len > D || b.len() != 80 + len * 64 {
            return Err(HistoryFailure::StoredIntegrity);
        }
        let mut config = self.config_bytes()?;
        config[44..46].copy_from_slice(&to_u16(len)?.to_le_bytes());
        if b[24..72] != config {
            return Err(HistoryFailure::StoredIntegrity);
        }
        *s = State::EMPTY;
        s.dataset.copy_from_slice(&b[8..24]);
        s.len = len;
        for d in 0..len {
            let b = &b[80 + d * 64..80 + (d + 1) * 64];
            let key = b[0];
            let epoch = u32at(b, 4);
            if key == 255
                || epoch == 0
                || b[1..4] != [0; 3]
                || b[48..64] != [0; 16]
                || d > 0 && s.domains[d - 1].domain >= key
            {
                return Err(HistoryFailure::StoredIntegrity);
            }
            s.domains[d] = HistoryDomain {
                domain: key,
                epoch,
                oldest_id: u64at(b, 32),
                newest_id: u64at(b, 40),
            };
            s.ack[d] = u64at(b, 16);
            s.count[d] = u32at(b, 24);
            s.tail[d] = u32at(b, 28);
            if s.count[d] > to_u32(R)?
                || s.tail[d] > self.config.slots_per_domain
                || s.count[d] > s.tail[d]
                || s.ack[d] > s.domains[d].newest_id
                || (s.count[d] == 0) != (s.domains[d].newest_id == 0)
                || (s.count[d] == 0) != (s.domains[d].oldest_id == 0)
                || u64at(b, 8) != u64at(b, 40)
                || s.domains[d].oldest_id > s.domains[d].newest_id
            {
                return Err(HistoryFailure::StoredIntegrity);
            }
        }
        Ok(())
    }
    // Admission needs only fallible size/address conversions, not a second
    // complete header and body CRC on the foreground CONTROL stack.
    pub(crate) fn check_header(
        &self,
        r: &Record<B>,
        d: usize,
        slot: u32,
    ) -> Result<(), HistoryFailure> {
        to_u16(r.len)?;
        to_u16(B)?;
        to_u16(R)?;
        to_u16(self.data.len)?;
        to_u32(d)?
            .checked_mul(self.config.slots_per_domain)
            .and_then(|base| base.checked_add(slot))
            .ok_or(HistoryFailure::Capacity)?;
        if r.len > B {
            return Err(HistoryFailure::Capacity);
        }
        Ok(())
    }
    pub(crate) fn header(
        &self,
        r: &Record<B>,
        d: usize,
        slot: u32,
    ) -> Result<[u8; 80], HistoryFailure> {
        self.check_header(r, d, slot)?;
        let mut h = [0; 80];
        h[..8].copy_from_slice(b"HPGREC01");
        h[8..24].copy_from_slice(&self.data.dataset);
        h[24] = r.domain;
        h[28..32].copy_from_slice(&r.epoch.to_le_bytes());
        h[32..40].copy_from_slice(&r.id.to_le_bytes());
        h[40..42].copy_from_slice(&r.kind.to_le_bytes());
        h[42..44].copy_from_slice(&r.schema.to_le_bytes());
        h[44..52].copy_from_slice(&r.utc.to_le_bytes());
        h[52..54].copy_from_slice(&r.flags.to_le_bytes());
        h[54..56].copy_from_slice(&to_u16(r.len)?.to_le_bytes());
        h[56..64].copy_from_slice(&r.parent.to_le_bytes());
        h[64..68].copy_from_slice(&r.crc.to_le_bytes());
        h[68..72]
            .copy_from_slice(&(to_u32(d)? * self.config.slots_per_domain + slot).to_le_bytes());
        h[76..80].copy_from_slice(&crc(&[&self.config_bytes()?]).to_le_bytes());
        let check = crc(&[b"HPG1", &h[..72], &h[76..80], &r.body[..r.len]]);
        h[72..76].copy_from_slice(&check.to_le_bytes());
        Ok(h)
    }
}
use super::{HistoryClock, HistorySchema, PagedHistoryStore};
use device_storage::{
    CommitState,
    persistence::{Medium, SnapshotStore},
};
impl<
    S: SnapshotStore,
    L: Medium,
    V: HistorySchema,
    C: HistoryClock,
    const D: usize,
    const R: usize,
    const B: usize,
    const N: usize,
> PagedHistoryStore<S, L, V, C, D, R, B, N>
{
    fn format(&self) -> Format<'_, D, R, B, N> {
        Format {
            config: self.config,
            geometry: self.geometry,
            stride: self.stride,
            data: &self.data,
        }
    }
    fn config_bytes(&self) -> Result<[u8; 48], HistoryFailure> {
        self.format().config_bytes()
    }
    pub(super) fn encode(
        &self,
        data: &State<D>,
        out: &mut [u8; N],
    ) -> Result<usize, HistoryFailure> {
        self.format().encode(data, out)
    }
    pub(super) fn decode(&self, b: &[u8]) -> Result<State<D>, HistoryFailure> {
        self.format().decode(b)
    }
    pub(super) fn header(
        &self,
        r: &Record<B>,
        d: usize,
        slot: u32,
    ) -> Result<[u8; 80], HistoryFailure> {
        self.format().header(r, d, slot)
    }
    pub(super) fn load_record(
        &self,
        d: usize,
        slot: u32,
    ) -> Result<Option<Record<B>>, HistoryFailure> {
        let at = self.offset(d, slot)?;
        let mut marker = [0; 256];
        let unit = self.geometry.write_size as usize;
        self.read_bytes(
            at + self.stride - self.geometry.write_size,
            &mut marker[..unit],
        )?;
        if marker[..unit].iter().any(|b| *b != 0xa5) {
            return Ok(None);
        }
        let mut h = [0; 80];
        self.read_bytes(at, &mut h)?;
        if &h[..8] != b"HPGREC01"
            || h[8..24] != self.data.dataset
            || h[24] != self.data.domains[d].domain
            || h[25..28] != [0; 3]
            || u32at(&h, 28) != self.data.domains[d].epoch
            || u32at(&h, 68) != to_u32(d)? * self.config.slots_per_domain + slot
            || u32at(&h, 76) != crc(&[&self.config_bytes()?])
        {
            return Err(self.corrupt());
        }
        let len = u16at(&h, 54) as usize;
        if len > B {
            return Err(self.corrupt());
        }
        let mut r = Record::<B>::EMPTY;
        r.domain = h[24];
        r.epoch = u32at(&h, 28);
        r.id = u64at(&h, 32);
        r.kind = u16at(&h, 40);
        r.schema = u16at(&h, 42);
        r.utc = u64at(&h, 44).cast_signed();
        r.flags = u16at(&h, 52);
        r.len = len;
        r.parent = u64at(&h, 56);
        r.crc = u32at(&h, 64);
        self.read_bytes(at + 80, &mut r.body[..len])?;
        if u32at(&h, 72) != crc(&[b"HPG1", &h[..72], &h[76..80], &r.body[..len]])
            || !self.ff(
                at + 80 + to_u32(len)?,
                self.stride - self.geometry.write_size - 80 - to_u32(len)?,
            )?
        {
            return Err(self.corrupt());
        }
        r.stored()
            .verify(CommitState::Committed, r.crc)
            .map_err(|_| self.corrupt())?;
        self.schema
            .validate(r.stored(), r.parent)
            .map_err(|_| self.corrupt())?;
        Ok(Some(r))
    }
}

#[cfg(test)]
mod compact_state_tests {
    use super::*;
    #[test]
    fn test_should_prevalidate_header_without_constructing_or_hashing_it() {
        let state = State::<1>::EMPTY;
        let context = Format::<1, 255, 16, 144> {
            config: LogConfig {
                namespace: [2; 16],
                log_start: 8192,
                slots_per_domain: 259,
            },
            geometry: Geometry {
                capacity: 1_048_576,
                write_size: 256,
                erase_size: 4096,
            },
            stride: 512,
            data: &state,
        };
        let mut record = Record::<16>::EMPTY;
        record.len = 16;
        assert_eq!(context.check_header(&record, 0, 258), Ok(()));
        assert!(context.header(&record, 0, 258).is_ok());
        record.len = 17;
        assert_eq!(
            context.check_header(&record, 0, 258),
            Err(HistoryFailure::Capacity)
        );
        assert_eq!(
            context.header(&record, 0, 258),
            Err(HistoryFailure::Capacity)
        );
        record.len = 16;
        assert_eq!(
            context.check_header(&record, usize::MAX, u32::MAX),
            Err(HistoryFailure::Invalid)
        );
    }
    #[test]
    fn test_should_keep_wire_high_mismatch_rejected_without_duplicate_directory() {
        let mut state = State::<1>::EMPTY;
        state.dataset = [1; 16];
        state.len = 1;
        state.domains[0] = HistoryDomain {
            domain: 0,
            epoch: 1,
            oldest_id: 0,
            newest_id: 0,
        };
        let context = Format::<1, 255, 474, 144> {
            config: LogConfig {
                namespace: [2; 16],
                log_start: 8192,
                slots_per_domain: 259,
            },
            geometry: Geometry {
                capacity: 1_048_576,
                write_size: 256,
                erase_size: 4096,
            },
            stride: 1024,
            data: &state,
        };
        let mut raw = [0; 144];
        assert_eq!(context.encode(&state, &mut raw), Ok(144));
        raw[88..96].copy_from_slice(&1u64.to_le_bytes());
        assert!(matches!(
            context.decode(&raw),
            Err(HistoryFailure::StoredIntegrity)
        ));
        assert!(
            core::mem::size_of::<State<6>>() <= 264,
            "duplicate high-ID array retained"
        );
    }
}
