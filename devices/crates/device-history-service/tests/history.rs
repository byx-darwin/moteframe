//! Non-durable synthetic `RecordStore` fixtures; no real Flash or phone persistence proof.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use core::{cell::Cell, num::NonZeroU64};
use device_history_service::*;
use device_protocol::{
    DataKind, Frame, Header, HistoryDomain, HistoryFilter, HistoryRequest, HistorySnapshot, Kind,
    ModuleEntry, SharedDataLane,
};
use device_services::{ServiceHandler, SessionError};
use device_storage::{CommitState, StoredRecord};
const DATASET: [u8; 16] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];
const DOMAINS: [HistoryDomain; 1] = [HistoryDomain {
    domain: 32,
    epoch: 3,
    oldest_id: 4,
    newest_id: 4,
}];
const CHANGED: [HistoryDomain; 1] = [HistoryDomain {
    domain: 32,
    epoch: 4,
    oldest_id: 4,
    newest_id: 4,
}];
const RECORD: [u8; 42] = [
    1, 0, 0, 0, 2, 0, 3, 0, 0, 0, 4, 0, 0, 0, 0, 0, 0, 0, 32, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 4, 0, 60, 0, 0, 1, 0xbd, 0x1a, 0x16, 0x3a,
];
struct Access(Cell<bool>);
impl HistoryAccess for Access {
    fn module_allowed(&self) -> bool {
        self.0.get()
    }
    fn allowed(&self, _: [u8; 16], _: Option<u8>, _: HistoryOperation) -> bool {
        self.0.get()
    }
}
struct Memory<'a> {
    prepare_calls: Option<&'a Cell<u32>>,
    prefetch_calls: Option<&'a Cell<u32>>,
    dataset_id: [u8; 16],
    directory_fault: Option<&'a Cell<bool>>,
    records: Vec<u64>,
    committed: u64,
    proof: Vec<u64>,
    filter: Option<HistoryFilter>,
    lease: Option<StoreLease>,
    epoch_changed: Cell<bool>,
    epoch_signal: Option<&'a Cell<bool>>,
    renew_failure: Option<&'a Cell<bool>>,
    domain: HistoryDomain,
    domains: Option<&'a [HistoryDomain]>,
    storage_error: Option<HistoryFailure>,
    corrupt: bool,
    pending: bool,
    renewals: u32,
    closes: u32,
    revoke_read: Option<&'a Access>,
}
impl Memory<'_> {
    fn new() -> Self {
        Self {
            prepare_calls: None,
            prefetch_calls: None,
            dataset_id: DATASET,
            directory_fault: None,
            records: vec![4],
            committed: 0,
            proof: Vec::new(),
            filter: None,
            lease: None,
            epoch_changed: Cell::new(false),
            epoch_signal: None,
            renew_failure: None,
            domain: DOMAINS[0],
            domains: None,
            storage_error: None,
            corrupt: false,
            pending: false,
            renewals: 0,
            closes: 0,
            revoke_read: None,
        }
    }
    fn permit(access: &dyn HistoryAccess) -> Result<(), HistoryFailure> {
        if access.module_allowed() && access.allowed(DATASET, Some(32), HistoryOperation::Read) {
            Ok(())
        } else {
            Err(HistoryFailure::Denied)
        }
    }
    fn live(&self, lease: StoreLease) -> Result<(), HistoryFailure> {
        if self.lease == Some(lease) {
            Ok(())
        } else {
            Err(HistoryFailure::CursorExpired)
        }
    }
}
impl RecordStore for Memory<'_> {
    fn prepare(
        &mut self,
        _: Preparation,
        _: u64,
        _: &dyn HistoryAccess,
    ) -> Result<bool, HistoryFailure> {
        if let Some(calls) = self.prepare_calls {
            calls.set(calls.get() + 1);
        }
        Ok(true)
    }

    fn prefetch(
        &self,
        _: StoreLease,
        _: u64,
        _: &dyn HistoryAccess,
    ) -> Result<bool, HistoryFailure> {
        if let Some(calls) = self.prefetch_calls {
            calls.set(calls.get() + 1);
        }
        Ok(true)
    }
    fn directory(&self) -> Result<Directory<'_>, HistoryFailure> {
        if self.directory_fault.is_some_and(Cell::get) {
            return Err(HistoryFailure::Storage);
        }
        Ok(Directory {
            dataset: self.dataset_id,
            domains: if self.epoch_changed.get() || self.epoch_signal.is_some_and(Cell::get) {
                &CHANGED
            } else {
                self.domains
                    .unwrap_or_else(|| core::slice::from_ref(&self.domain))
            },
        })
    }
    fn open(
        &mut self,
        snapshot: u32,
        filter: HistoryFilter,
        _: u64,
        access: &dyn HistoryAccess,
    ) -> Result<StoreLease, HistoryFailure> {
        Self::permit(access)?;
        if let Some(error) = self.storage_error {
            return Err(error);
        }
        if self.lease.is_some() {
            return Err(HistoryFailure::Busy);
        }
        if filter.domain != 32 {
            return Err(HistoryFailure::NotFound);
        }
        let lease = StoreLease {
            handle: u64::from(snapshot),
            snapshot: HistorySnapshot {
                snapshot,
                epoch: 3,
                first_id: self.records.first().copied().unwrap_or(0),
                last_id: self.records.last().copied().unwrap_or(0),
                count: u32::try_from(self.records.len()).unwrap(),
                lease_s: 60,
            },
            committed_id: self.committed,
        };
        self.proof.clear();
        self.filter = Some(filter);
        self.lease = Some(lease);
        Ok(lease)
    }
    fn plan(
        &mut self,
        lease: StoreLease,
        after: u64,
        max: u16,
        _: u64,
        access: &dyn HistoryAccess,
    ) -> Result<ReadPlan, HistoryFailure> {
        Self::permit(access)?;
        self.live(lease)?;
        if after > lease.snapshot.last_id {
            return Err(HistoryFailure::Invalid);
        }
        let mut ids = [0; 16];
        let mut count = 0;
        let mut eof = true;
        for id in self
            .records
            .iter()
            .copied()
            .filter(|id| *id > after && *id <= lease.snapshot.last_id)
        {
            if count == usize::from(max) {
                eof = false;
                break;
            }
            ids[count] = id;
            count += 1;
        }
        Ok(ReadPlan {
            ids,
            count: u16::try_from(count).unwrap(),
            end_after_id: if count == 0 { after } else { ids[count - 1] },
            eof,
        })
    }
    fn read(
        &self,
        lease: StoreLease,
        id: u64,
        out: &mut [u8],
        access: &dyn HistoryAccess,
    ) -> Result<RecordContent, HistoryFailure> {
        Self::permit(access)?;
        self.live(lease)?;
        if !self.records.contains(&id) {
            return Err(HistoryFailure::NotFound);
        }
        if out.len() < 4 {
            return Err(HistoryFailure::Capacity);
        }
        out[..4].copy_from_slice(&[60, 0, 0, 1]);
        let stored = StoredRecord {
            domain: 32,
            epoch: 3,
            record_id: id,
            record_type: 32,
            schema: 1,
            utc_ms: 0,
            flags: 0,
            payload: &out[..4],
        };
        let crc = stored.crc().unwrap();
        if let Some(access) = self.revoke_read {
            access.0.set(false);
        }
        Ok(RecordContent {
            domain: 32,
            epoch: 3,
            record_id: id,
            record_type: 32,
            schema: 1,
            utc_ms: 0,
            flags: 0,
            length: 4,
            commit: if self.pending {
                CommitState::Pending
            } else {
                CommitState::Committed
            },
            stored_crc: if self.corrupt { crc ^ 1 } else { crc },
        })
    }
    fn validate_record(
        &self,
        _: StoreLease,
        record: StoredRecord<'_>,
    ) -> Result<(), HistoryFailure> {
        if record.domain == 32
            && record.record_type == 32
            && record.schema == 1
            && record.payload.len() == 4
        {
            Ok(())
        } else {
            Err(HistoryFailure::Invalid)
        }
    }
    fn record_admitted(
        &mut self,
        lease: StoreLease,
        id: u64,
        access: &dyn HistoryAccess,
    ) -> Result<(), HistoryFailure> {
        Self::permit(access)?;
        self.live(lease)?;
        if !self.records.contains(&id) {
            return Err(HistoryFailure::NotFound);
        }
        if !self.proof.contains(&id) {
            self.proof.push(id);
        }
        Ok(())
    }
    fn ack_atomic(
        &mut self,
        lease: StoreLease,
        filter: HistoryFilter,
        epoch: u32,
        upto: u64,
        _: u64,
        access: &dyn HistoryAccess,
    ) -> Result<u64, HistoryFailure> {
        Self::permit(access)?;
        if !access.allowed(DATASET, Some(32), HistoryOperation::Acknowledge) {
            return Err(HistoryFailure::Denied);
        }
        self.live(lease)?;
        if epoch != 3 {
            return Err(HistoryFailure::Conflict);
        }
        if filter.filter_flags != 0 {
            return Err(HistoryFailure::InvalidState);
        }
        if upto <= self.committed {
            return Ok(self.committed);
        }
        if !self.records.contains(&upto)
            || upto > lease.snapshot.last_id
            || self
                .records
                .iter()
                .any(|id| *id > self.committed && *id <= upto && !self.proof.contains(id))
        {
            return Err(HistoryFailure::Invalid);
        }
        if let Some(error) = self.storage_error {
            return Err(error);
        }
        self.committed = upto;
        Ok(self.committed)
    }
    fn count(
        &self,
        filter: HistoryFilter,
        access: &dyn HistoryAccess,
    ) -> Result<(u32, u32), HistoryFailure> {
        Self::permit(access)?;
        if filter.domain != 32 {
            return Err(HistoryFailure::NotFound);
        }
        Ok((3, u32::try_from(self.records.len()).unwrap()))
    }
    fn renew(
        &mut self,
        lease: StoreLease,
        _: u64,
        access: &dyn HistoryAccess,
    ) -> Result<(), HistoryFailure> {
        if self.renew_failure.is_some_and(Cell::get) {
            return Err(HistoryFailure::Storage);
        }
        Self::permit(access)?;
        self.live(lease)?;
        self.renewals += 1;
        Ok(())
    }
    fn close(&mut self, lease: StoreLease) -> Result<(), HistoryFailure> {
        self.live(lease)?;
        self.lease = None;
        self.closes += 1;
        Ok(())
    }
}
fn metadata() -> ModuleEntry {
    ModuleEntry {
        module: 0x10,
        major: 1,
        minor: 2,
        availability: 1,
        features: 0,
        input_max: 512,
        output_max: 512,
    }
}
fn filter() -> HistoryFilter {
    HistoryFilter {
        domain: 32,
        filter_flags: 0,
        start_utc_ms: 0,
        end_utc_ms: 0,
        record_type: 0,
        parent_session: 0,
    }
}
fn frame(opcode: u8, bytes: &[u8]) -> Frame<'_> {
    Frame {
        header: Header {
            kind: Kind::Request,
            module: 0x10,
            opcode,
            message_id: 1,
            offset: 0,
            total_length: u16::try_from(bytes.len()).unwrap(),
        },
        payload: bytes,
    }
}
type Handler<'a, 'b> = HistoryHandler<'a, Memory<'b>, 1, 90>;
fn handler<'a, 'b>(
    memory: &'a mut Memory<'b>,
    access: &'a Access,
    lane: &'a SharedDataLane,
) -> Handler<'a, 'b> {
    HistoryHandler::new(metadata(), memory, access, lane, 128, true).unwrap()
}
fn run(service: &mut Handler<'_, '_>, request: HistoryRequest) -> Vec<u8> {
    let mut input = [0; 128];
    let n = request.encode(&mut input).unwrap();
    let mut out = [0xa5; 512];
    let n = service
        .dispatch(frame(request.opcode(), &input[..n]), &mut out)
        .unwrap();
    out[..n].to_vec()
}
fn open(service: &mut Handler<'_, '_>) {
    let response = run(service, HistoryRequest::Open(filter()));
    assert_eq!(
        response,
        [
            0, 0, 0, 0, 1, 0, 0, 0, 3, 0, 0, 0, 4, 0, 0, 0, 0, 0, 0, 0, 4, 0, 0, 0, 0, 0, 0, 0, 1,
            0, 0, 0, 60, 0
        ]
    );
}
fn read(service: &mut Handler<'_, '_>, after: u64, max: u16) -> u16 {
    let response = run(
        service,
        HistoryRequest::Read {
            snapshot: 1,
            after_id: after,
            max_records: max,
        },
    );
    assert_eq!(&response[..4], &[0; 4]);
    u16::from_le_bytes(response[4..6].try_into().unwrap())
}
fn admit_read(service: &mut Handler<'_, '_>, after: u64, max: u16) {
    let request = HistoryRequest::Read {
        snapshot: 1,
        after_id: after,
        max_records: max,
    };
    let mut input = [0; 14];
    request.encode(&mut input).unwrap();
    let mut response = [0; 17];
    response[4..6].copy_from_slice(&1u16.to_le_bytes());
    let end: u64 = if after == 2 { 3 } else { 4 };
    response[6..14].copy_from_slice(&end.to_le_bytes());
    response[14] = 1;
    response[15..17].copy_from_slice(&1u16.to_le_bytes());
    service
        .response_admitted(frame(3, &input), &response)
        .unwrap();
}
fn lane() -> SharedDataLane {
    SharedDataLane::new(NonZeroU64::new(1).unwrap())
}

#[test]
fn maximum_directory_reply_preserves_all_domains_and_capacity_pagination() {
    let domains = core::array::from_fn::<_, 8, _>(|index| HistoryDomain {
        domain: u8::try_from(index).unwrap(),
        epoch: 1,
        oldest_id: 0,
        newest_id: 0,
    });
    let access = Access(Cell::new(true));
    let lane = lane();
    let mut memory = Memory::new();
    memory.domains = Some(&domains);
    let mut service =
        HistoryHandler::<_, 8, 90>::new(metadata(), &mut memory, &access, &lane, 512, true)
            .unwrap();
    let request = HistoryRequest::Enumerate { start: 0, max: 8 };
    let mut input = [0; 2];
    let size = request.encode(&mut input).unwrap();
    for capacity in [189, 190, 512] {
        let mut out = [0xa5; 512];
        let length = service
            .dispatch(frame(1, &input[..size]), &mut out[..capacity])
            .unwrap();
        let device_protocol::HistoryResponse::Enumerate(page) =
            device_protocol::HistoryResponse::decode(1, &out[..length]).unwrap()
        else {
            panic!("expected directory page")
        };
        let count = if capacity == 189 { 7 } else { 8 };
        assert_eq!(length, 22 + count * HistoryDomain::LENGTH);
        assert_eq!(page.next, if count == 8 { 255 } else { 7 });
        for (index, expected) in domains[..count].iter().enumerate() {
            assert_eq!(page.domain(index), Some(*expected));
        }
        assert!(out[length..].iter().all(|byte| *byte == 0xa5));
    }
}

#[test]
fn frozen_directory_open_read_record_done_and_actual_dataset() {
    let access = Access(Cell::new(true));
    let lane = lane();
    lane.with_mut(|owner| {
        let lease = owner.acquire(DataKind::Raw)?;
        owner.release(lease)
    })
    .unwrap();
    let mut memory = Memory::new();
    let mut service = handler(&mut memory, &access, &lane);
    assert_eq!(service.dataset(), Ok(DATASET));
    let page = run(&mut service, HistoryRequest::Enumerate { start: 0, max: 8 });
    assert_eq!(page.len(), 43);
    assert_eq!(&page[4..20], &DATASET);
    assert_eq!(&page[20..23], &[255, 1, 32]);
    open(&mut service);
    let stream = read(&mut service, 0, 1);
    assert_eq!(stream, 2);
    let request = HistoryRequest::Read {
        snapshot: 1,
        after_id: 0,
        max_records: 1,
    };
    let mut input = [0; 14];
    request.encode(&mut input).unwrap();
    let mut response = [0; 17];
    response[4..6].copy_from_slice(&stream.to_le_bytes());
    response[6..14].copy_from_slice(&4u64.to_le_bytes());
    response[14] = 1;
    response[15..17].copy_from_slice(&1u16.to_le_bytes());
    service
        .response_admitted(frame(3, &input), &response)
        .unwrap();
    service.grant(DataKind::History, stream, 1).unwrap();
    let mut out = [0xa5; 128];
    let event = service.next_data(&mut out).unwrap().unwrap();
    assert_eq!(event.message_id, 1);
    assert_eq!(&out[..event.length], &RECORD);
    service.finish_data(event.message_id).unwrap();
    let n = service.done(&mut out).unwrap().unwrap();
    assert_eq!(&out[..n], &[2, 0, 1, 0, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    service.done_admitted(stream).unwrap();
    assert_eq!(lane.available(), Ok(true));
}
#[test]
fn read_reply_and_cumulative_credit_gate_all_data_and_retry_same_event() {
    let access = Access(Cell::new(true));
    let lane = lane();
    let mut memory = Memory::new();
    let mut service = handler(&mut memory, &access, &lane);
    open(&mut service);
    let stream = read(&mut service, 0, 1);
    let mut out = [0; 128];
    assert_eq!(service.next_data(&mut out), Ok(None));
    admit_read(&mut service, 0, 1);
    assert_eq!(service.next_data(&mut out), Ok(None));
    service.grant(DataKind::History, stream, 1).unwrap();
    service.grant(DataKind::History, stream, 1).unwrap();
    assert_eq!(
        service.grant(DataKind::History, stream, 0),
        Err(HistoryFailure::Conflict)
    );
    assert_eq!(
        service.grant(DataKind::Raw, stream, 2),
        Err(HistoryFailure::Identity)
    );
    let first = service.next_data(&mut out).unwrap().unwrap();
    let bytes = out;
    assert_eq!(service.next_data(&mut out), Ok(Some(first)));
    assert_eq!(out, bytes);
    assert_eq!(
        service.finish_data(first.message_id + 1),
        Err(HistoryFailure::Identity)
    );
    service.finish_data(first.message_id).unwrap();
}
#[test]
fn stored_crc_and_commit_are_verified_before_any_credit_or_data_identifier() {
    for pending in [false, true] {
        let access = Access(Cell::new(true));
        let lane = lane();
        let mut memory = Memory::new();
        memory.corrupt = !pending;
        memory.pending = pending;
        let mut service = handler(&mut memory, &access, &lane);
        open(&mut service);
        let stream = read(&mut service, 0, 1);
        admit_read(&mut service, 0, 1);
        service.grant(DataKind::History, stream, 1).unwrap();
        let mut out = [0xa5; 128];
        assert_eq!(service.next_data(&mut out), Ok(None));
        let n = service.done(&mut out).unwrap().unwrap();
        assert_eq!(
            &out[n - 4..n],
            if pending {
                &[9, 0, 0, 0]
            } else {
                &[18, 0, 2, 255]
            }
        );
        service.done_admitted(stream).unwrap();
        let lease = lane.with_mut(|owner| owner.acquire(DataKind::Raw)).unwrap();
        assert_eq!(lane.with_mut(|owner| owner.begin_message(lease)), Ok(1));
    }
}
#[test]
fn every_filter_bit_is_read_only_even_if_it_happens_to_select_every_record() {
    for flags in [1, 2, 4, 7] {
        let access = Access(Cell::new(true));
        let lane = lane();
        let mut memory = Memory::new();
        let mut service = handler(&mut memory, &access, &lane);
        let mut selected = filter();
        selected.filter_flags = flags;
        if flags & 1 != 0 {
            selected.start_utc_ms = -1;
            selected.end_utc_ms = 1;
        }
        if flags & 2 != 0 {
            selected.record_type = 32;
        }
        if flags & 4 != 0 {
            selected.parent_session = 1;
        }
        assert_eq!(
            &run(&mut service, HistoryRequest::OpenV2(selected))[..4],
            &[0; 4]
        );
        assert_eq!(
            run(
                &mut service,
                HistoryRequest::Ack {
                    snapshot: 1,
                    epoch: 3,
                    upto_id: 4
                }
            ),
            [4, 0, 5, 255]
        );
    }
}
#[test]
fn admitted_events_do_not_ack_and_atomic_backend_rejects_omitted_real_records() {
    let access = Access(Cell::new(true));
    let lane = lane();
    let mut memory = Memory::new();
    memory.records = vec![1, 2, 3];
    memory.domain.oldest_id = 1;
    memory.domain.newest_id = 3;
    {
        let mut service = handler(&mut memory, &access, &lane);
        assert_eq!(
            &run(&mut service, HistoryRequest::Open(filter()))[..4],
            &[0; 4]
        );
        let stream = read(&mut service, 2, 1);
        admit_read(&mut service, 2, 1);
        service.grant(DataKind::History, stream, 1).unwrap();
        let mut out = [0; 128];
        let event = service.next_data(&mut out).unwrap().unwrap();
        service.finish_data(event.message_id).unwrap();
        assert_eq!(
            run(
                &mut service,
                HistoryRequest::Ack {
                    snapshot: 1,
                    epoch: 3,
                    upto_id: 3
                }
            ),
            [1, 0, 0, 0]
        );
    }
    assert_eq!(memory.committed, 0);
    assert_eq!(memory.proof, vec![3]);
}
#[test]
fn explicit_ack_persists_only_after_complete_actual_prefix_and_renews_lease() {
    let access = Access(Cell::new(true));
    let lane = lane();
    let mut memory = Memory::new();
    {
        let mut service = handler(&mut memory, &access, &lane);
        open(&mut service);
        assert_eq!(
            run(
                &mut service,
                HistoryRequest::Ack {
                    snapshot: 1,
                    epoch: 3,
                    upto_id: 4
                }
            ),
            [1, 0, 0, 0]
        );
        let stream = read(&mut service, 0, 1);
        admit_read(&mut service, 0, 1);
        service.grant(DataKind::History, stream, 1).unwrap();
        let mut out = [0; 128];
        let event = service.next_data(&mut out).unwrap().unwrap();
        service.finish_data(event.message_id).unwrap();
        assert_eq!(
            run(
                &mut service,
                HistoryRequest::Ack {
                    snapshot: 1,
                    epoch: 4,
                    upto_id: 4
                }
            ),
            [11, 0, 0, 0]
        );
        assert_eq!(
            run(
                &mut service,
                HistoryRequest::Ack {
                    snapshot: 1,
                    epoch: 3,
                    upto_id: 4
                }
            ),
            [0, 0, 0, 0, 4, 0, 0, 0, 0, 0, 0, 0]
        );
    }
    assert_eq!(memory.committed, 4);
    assert!(memory.renewals >= 2);
}
#[test]
fn cancel_half_message_waits_for_drain_without_new_credit_and_preserves_snapshot() {
    let access = Access(Cell::new(true));
    let lane = lane();
    let mut memory = Memory::new();
    let mut service = handler(&mut memory, &access, &lane);
    open(&mut service);
    let stream = read(&mut service, 0, 1);
    admit_read(&mut service, 0, 1);
    service.grant(DataKind::History, stream, 1).unwrap();
    let mut out = [0; 128];
    let event = service.next_data(&mut out).unwrap().unwrap();
    let request = HistoryRequest::CancelRead {
        snapshot: 1,
        stream_id: stream,
    };
    let mut input = [0; 6];
    request.encode(&mut input).unwrap();
    let cancel = frame(8, &input);
    assert_eq!(service.response_ready(cancel), Ok(false));
    assert_eq!(
        service.dispatch(cancel, &mut out),
        Err(SessionError::Deferred)
    );
    assert_eq!(lane.available(), Ok(false));
    assert_eq!(service.next_data(&mut out), Ok(Some(event)));
    service.finish_data(event.message_id).unwrap();
    assert_eq!(service.response_ready(cancel), Ok(true));
    assert_eq!(service.dispatch(cancel, &mut out), Ok(6));
    assert_eq!(&out[..6], &[0, 0, 0, 0, 1, 0]);
    assert_eq!(lane.available(), Ok(true));
    assert_eq!(service.done(&mut out), Ok(None));
    let next = read(&mut service, 0, 1);
    assert_ne!(next, stream);
}
#[test]
fn zero_barrier_close_and_cancel_do_not_invent_data_or_ack() {
    let access = Access(Cell::new(true));
    let lane = lane();
    let mut memory = Memory::new();
    {
        let mut service = handler(&mut memory, &access, &lane);
        open(&mut service);
        let stream = read(&mut service, 0, 1);
        assert_eq!(
            run(
                &mut service,
                HistoryRequest::CancelRead {
                    snapshot: 1,
                    stream_id: stream
                }
            ),
            [0; 6]
        );
        assert_eq!(
            run(&mut service, HistoryRequest::Close { snapshot: 1 }),
            [0; 6]
        );
    }
    assert_eq!(memory.committed, 0);
    assert_eq!(memory.closes, 1);
}
#[test]
fn timeout_epoch_change_and_cccd_close_end_the_actual_shared_lane() {
    let access = Access(Cell::new(true));
    let lane = lane();
    let mut memory = Memory::new();
    let mut service = handler(&mut memory, &access, &lane);
    open(&mut service);
    let stream = read(&mut service, 0, 1);
    admit_read(&mut service, 0, 1);
    service.grant(DataKind::History, stream, 1).unwrap();
    let mut out = [0; 128];
    service.next_data(&mut out).unwrap().unwrap();
    let mut input = [0; 6];
    HistoryRequest::CancelRead {
        snapshot: 1,
        stream_id: stream,
    }
    .encode(&mut input)
    .unwrap();
    assert_eq!(service.response_ready(frame(8, &input)), Ok(false));
    service.advance(9999).unwrap();
    assert_eq!(service.advance(10000), Err(HistoryFailure::Timeout));
    assert!(service.must_end_session());
    assert_eq!(lane.available(), Ok(false));
    assert_eq!(service.connection_closed(), Ok(()));
}
#[test]
fn lease_expiry_and_epoch_conflicts_are_explicit_and_snapshot_ids_do_not_repeat() {
    let access = Access(Cell::new(true));
    let lane = lane();
    let mut memory = Memory::new();
    let mut service = handler(&mut memory, &access, &lane);
    open(&mut service);
    assert_eq!(
        run(
            &mut service,
            HistoryRequest::Read {
                snapshot: 1,
                after_id: 5,
                max_records: 1
            }
        ),
        [1, 0, 0, 0]
    );
    service.advance(60000).unwrap();
    assert_eq!(
        run(
            &mut service,
            HistoryRequest::Read {
                snapshot: 1,
                after_id: 0,
                max_records: 1
            }
        ),
        [12, 0, 0, 0]
    );
    let response = run(&mut service, HistoryRequest::Open(filter()));
    assert_eq!(&response[4..8], &2u32.to_le_bytes());
}
#[test]
fn raw_and_history_share_one_external_exclusive_owner_and_no_backend_means_no_success() {
    let access = Access(Cell::new(true));
    let lane = lane();
    let raw = lane.with_mut(|owner| owner.acquire(DataKind::Raw)).unwrap();
    let mut memory = Memory::new();
    let mut service = handler(&mut memory, &access, &lane);
    open(&mut service);
    assert_eq!(
        run(
            &mut service,
            HistoryRequest::Read {
                snapshot: 1,
                after_id: 0,
                max_records: 1
            }
        ),
        [6, 0, 0, 0]
    );
    lane.with_mut(|owner| owner.release(raw)).unwrap();
    assert_eq!(read(&mut service, 0, 1), 2);
}
#[test]
fn untrusted_initial_access_and_mid_read_revocation_do_not_expose_bytes() {
    let access = Access(Cell::new(false));
    let lane = lane();
    let mut memory = Memory::new();
    let mut service = handler(&mut memory, &access, &lane);
    assert_eq!(
        run(&mut service, HistoryRequest::Enumerate { start: 0, max: 1 }),
        [5, 0, 3, 255]
    );
    assert_eq!(service.dataset(), Err(HistoryFailure::Denied));
}
#[test]
fn ack_lease_renewal_failure_cannot_advance_the_durable_watermark() {
    let access = Access(Cell::new(true));
    let lane = lane();
    let fail = Cell::new(false);
    let mut memory = Memory::new();
    memory.renew_failure = Some(&fail);
    {
        let mut service = handler(&mut memory, &access, &lane);
        open(&mut service);
        let stream = read(&mut service, 0, 1);
        admit_read(&mut service, 0, 1);
        service.grant(DataKind::History, stream, 1).unwrap();
        let mut out = [0; 128];
        let event = service.next_data(&mut out).unwrap().unwrap();
        service.finish_data(event.message_id).unwrap();
        fail.set(true);
        assert_eq!(
            run(
                &mut service,
                HistoryRequest::Ack {
                    snapshot: 1,
                    epoch: 3,
                    upto_id: 4
                }
            ),
            [9, 0, 0, 0]
        );
    }
    assert_eq!(memory.committed, 0);
}
#[test]
fn reentrant_shared_lane_busy_preserves_credit_and_does_not_create_a_done_error() {
    let access = Access(Cell::new(true));
    let lane = lane();
    let mut memory = Memory::new();
    let mut service = handler(&mut memory, &access, &lane);
    open(&mut service);
    let stream = read(&mut service, 0, 1);
    admit_read(&mut service, 0, 1);
    service.grant(DataKind::History, stream, 1).unwrap();
    let mut out = [0; 128];
    lane.with_mut(|_| {
        assert_eq!(service.next_data(&mut out), Err(HistoryFailure::Busy));
        Ok(())
    })
    .unwrap();
    assert_eq!(service.done(&mut out), Ok(None));
    assert!(service.next_data(&mut out).unwrap().is_some());
}
#[test]
fn read_reply_admission_rejects_wrong_snapshot_stream_count_or_cursor_binding() {
    let access = Access(Cell::new(true));
    let lane = lane();
    let mut memory = Memory::new();
    let mut service = handler(&mut memory, &access, &lane);
    open(&mut service);
    read(&mut service, 0, 1);
    let mut input = [0; 14];
    HistoryRequest::Read {
        snapshot: 1,
        after_id: 0,
        max_records: 1,
    }
    .encode(&mut input)
    .unwrap();
    assert_eq!(
        service.response_admitted(frame(3, &input), &[0; 17]),
        Err(SessionError::InvalidHandlerResponse)
    );
    assert_eq!(service.next_data(&mut [0; 128]), Ok(None));
}
#[test]
fn real_epoch_change_and_read_revocation_close_the_shared_connection_without_data() {
    let access = Access(Cell::new(true));
    let lane = lane();
    let signal = Cell::new(false);
    let mut memory = Memory::new();
    memory.epoch_signal = Some(&signal);
    let mut service = handler(&mut memory, &access, &lane);
    open(&mut service);
    signal.set(true);
    assert!(service.must_end_session());
    assert_eq!(lane.available(), Ok(false));
    service.connection_closed().unwrap();
    let lane = SharedDataLane::new(NonZeroU64::new(2).unwrap());
    let mut memory = Memory::new();
    memory.revoke_read = Some(&access);
    let mut service = handler(&mut memory, &access, &lane);
    open(&mut service);
    let stream = read(&mut service, 0, 1);
    admit_read(&mut service, 0, 1);
    service.grant(DataKind::History, stream, 1).unwrap();
    let mut out = [0xa5; 128];
    assert_eq!(service.next_data(&mut out), Err(HistoryFailure::Denied));
    assert_eq!(out, [0; 128]);
    assert!(service.must_end_session());
    service.connection_closed().unwrap();
}
#[test]
fn exhausted_shared_identity_space_is_limit_exceeded_and_never_busy_or_reused() {
    for messages in [false, true] {
        let access = Access(Cell::new(true));
        let lane = lane();
        if messages {
            let lease = lane.with_mut(|owner| owner.acquire(DataKind::Raw)).unwrap();
            for _ in 0..u16::MAX {
                lane.with_mut(|owner| {
                    let mid = owner.begin_message(lease)?;
                    owner.finish_message(lease, mid)
                })
                .unwrap();
            }
            lane.with_mut(|owner| owner.release(lease)).unwrap();
        } else {
            for _ in 0..u16::MAX {
                lane.with_mut(|owner| {
                    let lease = owner.acquire(DataKind::Raw)?;
                    owner.release(lease)
                })
                .unwrap();
            }
        }
        let mut memory = Memory::new();
        let mut service = handler(&mut memory, &access, &lane);
        open(&mut service);
        assert_eq!(
            run(
                &mut service,
                HistoryRequest::Read {
                    snapshot: 1,
                    after_id: 0,
                    max_records: 1
                }
            ),
            [7, 0, 0, 0]
        );
    }
}
#[test]
fn filtered_ack_still_requires_its_actual_unexpired_snapshot_identity() {
    let access = Access(Cell::new(true));
    let lane = lane();
    let mut memory = Memory::new();
    let mut service = handler(&mut memory, &access, &lane);
    let selected = HistoryFilter {
        filter_flags: 2,
        record_type: 32,
        ..filter()
    };
    assert_eq!(
        &run(&mut service, HistoryRequest::OpenV2(selected))[..4],
        &[0; 4]
    );
    assert_eq!(
        run(
            &mut service,
            HistoryRequest::Ack {
                snapshot: 2,
                epoch: 3,
                upto_id: 4
            }
        ),
        [12, 0, 0, 0]
    );
    service.advance(60000).unwrap();
    assert_eq!(
        run(
            &mut service,
            HistoryRequest::Ack {
                snapshot: 1,
                epoch: 3,
                upto_id: 4
            }
        ),
        [12, 0, 0, 0]
    );
}
#[test]
fn count_never_opens_a_snapshot_or_creates_an_ack_proof_and_empty_read_done_is_zero() {
    let access = Access(Cell::new(true));
    let lane = lane();
    let mut memory = Memory::new();
    {
        let mut service = handler(&mut memory, &access, &lane);
        assert_eq!(
            run(&mut service, HistoryRequest::Count(filter())),
            [0, 0, 0, 0, 3, 0, 0, 0, 1, 0, 0, 0]
        );
        assert_eq!(
            run(
                &mut service,
                HistoryRequest::Ack {
                    snapshot: 1,
                    epoch: 3,
                    upto_id: 4
                }
            ),
            [12, 0, 0, 0]
        );
        open(&mut service);
    }
    assert_eq!(memory.committed, 0);
    assert!(memory.proof.is_empty());
    let lane = SharedDataLane::new(NonZeroU64::new(2).unwrap());
    let mut memory = Memory::new();
    memory.records.clear();
    memory.domain.oldest_id = 0;
    memory.domain.newest_id = 0;
    let mut service = handler(&mut memory, &access, &lane);
    let opened = run(&mut service, HistoryRequest::Open(filter()));
    assert_eq!(&opened[12..32], &[0; 20]);
    let request = HistoryRequest::Read {
        snapshot: 1,
        after_id: 0,
        max_records: 1,
    };
    let response = run(&mut service, request);
    let mut input = [0; 14];
    request.encode(&mut input).unwrap();
    service
        .response_admitted(frame(3, &input), &response)
        .unwrap();
    let mut out = [0; 128];
    assert_eq!(service.next_data(&mut out), Ok(None));
    assert_eq!(service.done(&mut out), Ok(Some(16)));
    assert_eq!(
        &out[..16],
        &[1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
    );
    service.done_admitted(1).unwrap();
}
fn control_session(modules: &[ModuleEntry]) -> device_services::Session<'_> {
    use device_protocol::ProtocolLimits;
    use device_services::{BuildProfile, Config, DeviceInfo, Session};
    let mut session = Session::new(Config {
        session: 1,
        capability_rev: 1,
        profile: BuildProfile::Commercial,
        info: DeviceInfo {
            product_id: 0,
            fw_major: 0,
            fw_minor: 0,
            fw_patch: 0,
            build_id: 0,
        },
        modules,
        limits: ProtocolLimits {
            rx_frame_max: 20,
            tx_frame_max: 20,
            control_max: 512,
            data_max: 128,
            idle_ms: 2000,
            fragment_ms: 100,
            response_ms: 10000,
        },
    })
    .unwrap();
    session.control_notifications_enabled().unwrap();
    let mut hello = frame(1, &[1, 1, 128, 0, 128, 0, 3, 0]);
    hello.header.module = 0;
    session.dispatch(hello, &mut [0; 512]).unwrap();
    session
}
#[test]
fn session_deferred_cancel_emits_no_pending_and_reuses_neither_stream_nor_data_id() {
    let mut core = metadata();
    core.module = 0;
    core.minor = 2;
    let modules = [core, metadata()];
    let mut session = control_session(&modules);
    let access = Access(Cell::new(true));
    let lane = lane();
    let mut memory = Memory::new();
    {
        let mut service = handler(&mut memory, &access, &lane);
        let mut input = [0; 28];
        let mut out = [0; 512];
        let n = HistoryRequest::Open(filter()).encode(&mut input).unwrap();
        let mut request = frame(2, &input[..n]);
        request.header.message_id = 2;
        session
            .dispatch_with_handler(request, &mut service, &mut out)
            .unwrap();
        let n = HistoryRequest::Read {
            snapshot: 1,
            after_id: 0,
            max_records: 1,
        }
        .encode(&mut input)
        .unwrap();
        let mut read = frame(3, &input[..n]);
        read.header.message_id = 3;
        let n = session
            .dispatch_with_handler(read, &mut service, &mut out)
            .unwrap();
        assert_eq!(n, 17);
        service.response_admitted(read, &out[..n]).unwrap();
        service.grant(DataKind::History, 1, 1).unwrap();
        let mut data = [0; 128];
        let event = service.next_data(&mut data).unwrap().unwrap();
        assert_eq!(event.message_id, 1);
        device_protocol::HistoryRecord::decode(&data[..event.length], 128).unwrap();
        let n = HistoryRequest::CancelRead {
            snapshot: 1,
            stream_id: 1,
        }
        .encode(&mut input)
        .unwrap();
        let mut cancel = frame(8, &input[..n]);
        cancel.header.message_id = 4;
        for _ in 0..2 {
            out.fill(0xa5);
            assert_eq!(
                session.dispatch_with_handler(cancel, &mut service, &mut out),
                Err(SessionError::Deferred)
            );
            assert_eq!(out, [0; 512]);
        }
        service.finish_data(event.message_id).unwrap();
        assert_eq!(
            session.dispatch_with_handler(cancel, &mut service, &mut out),
            Ok(6)
        );
        assert_eq!(&out[..6], &[0, 0, 0, 0, 1, 0]);
        assert_eq!(
            session.dispatch_with_handler(cancel, &mut service, &mut out),
            Ok(6)
        );
        let n = HistoryRequest::Read {
            snapshot: 1,
            after_id: 0,
            max_records: 1,
        }
        .encode(&mut input)
        .unwrap();
        let mut read = frame(3, &input[..n]);
        read.header.message_id = 5;
        let n = session
            .dispatch_with_handler(read, &mut service, &mut out)
            .unwrap();
        assert_eq!(&out[4..6], &2u16.to_le_bytes());
        service.response_admitted(read, &out[..n]).unwrap();
        service.grant(DataKind::History, 2, 1).unwrap();
        let next = service.next_data(&mut data).unwrap().unwrap();
        assert_eq!(next.message_id, 2);
        service.finish_data(2).unwrap();
        assert_eq!(service.done(&mut out), Ok(Some(16)));
        service.done_admitted(2).unwrap();
        service.connection_closed().unwrap();
    }
    assert_eq!(memory.committed, 0);
    assert_eq!(memory.closes, 1);
}
#[test]
fn lease_deadline_overflow_releases_the_actual_backend_handle() {
    let access = Access(Cell::new(true));
    let lane = lane();
    let mut memory = Memory::new();
    {
        let mut service = handler(&mut memory, &access, &lane);
        service.advance(u64::MAX - 10).unwrap();
        assert_eq!(
            run(&mut service, HistoryRequest::Open(filter())),
            [17, 0, 0, 0]
        );
    }
    assert!(memory.lease.is_none());
    assert_eq!(memory.closes, 1);
}
#[test]
fn real_record_id_gaps_are_valid_when_every_actual_record_is_admitted() {
    let access = Access(Cell::new(true));
    let lane = lane();
    let mut memory = Memory::new();
    memory.records = vec![1, 3];
    memory.domain.oldest_id = 1;
    memory.domain.newest_id = 3;
    {
        let mut service = handler(&mut memory, &access, &lane);
        run(&mut service, HistoryRequest::Open(filter()));
        let request = HistoryRequest::Read {
            snapshot: 1,
            after_id: 0,
            max_records: 2,
        };
        let response = run(&mut service, request);
        let mut input = [0; 14];
        request.encode(&mut input).unwrap();
        service
            .response_admitted(frame(3, &input), &response)
            .unwrap();
        service.grant(DataKind::History, 1, 2).unwrap();
        let mut out = [0; 128];
        for id in [1, 3] {
            let event = service.next_data(&mut out).unwrap().unwrap();
            assert_eq!(
                device_protocol::HistoryRecord::decode(&out[..event.length], 128)
                    .unwrap()
                    .record_id,
                id
            );
            service.finish_data(event.message_id).unwrap();
        }
        assert_eq!(
            run(
                &mut service,
                HistoryRequest::Ack {
                    snapshot: 1,
                    epoch: 3,
                    upto_id: 3
                }
            ),
            [0, 0, 0, 0, 3, 0, 0, 0, 0, 0, 0, 0]
        );
    }
    assert_eq!(memory.committed, 3);
}
#[test]
fn zero_or_unverifiable_persistent_dataset_never_becomes_a_successful_identity() {
    let access = Access(Cell::new(true));
    let lane = lane();
    let mut memory = Memory::new();
    memory.dataset_id = [0; 16];
    assert_eq!(
        HistoryHandler::<_, 1, 90>::new(metadata(), &mut memory, &access, &lane, 128, true).err(),
        Some(ConfigError::Invalid)
    );
    let fault = Cell::new(false);
    let mut memory = Memory::new();
    memory.directory_fault = Some(&fault);
    let service = handler(&mut memory, &access, &lane);
    fault.set(true);
    assert!(service.must_end_session());
    assert_eq!(service.dataset(), Err(HistoryFailure::Closed));
}
#[test]
fn core_bridge_queries_do_not_change_credit_or_validate_old_generations() {
    let access = Access(Cell::new(true));
    let lane = lane();
    let mut memory = Memory::new();
    let mut service = handler(&mut memory, &access, &lane);
    open(&mut service);
    let stream = read(&mut service, 0, 1);
    assert_eq!(
        service.validate_grant(DataKind::History, stream, 1),
        Err(HistoryFailure::InvalidState)
    );
    assert!(!service.grant_replay_valid(stream, 1));
    admit_read(&mut service, 0, 1);
    assert_eq!(service.validate_grant(DataKind::History, stream, 1), Ok(()));
    assert!(!service.grant_replay_valid(stream, 1));
    service.grant(DataKind::History, stream, 1).unwrap();
    assert_eq!(
        service.validate_grant(DataKind::History, stream, 0),
        Err(HistoryFailure::Conflict)
    );
    assert!(service.grant_replay_valid(stream, 1));
    assert!(!service.grant_replay_valid(stream + 1, 1));
    assert!(!service.grant_replay_valid(stream, 2));
    let mut out = [0; 128];
    let event = service.next_data(&mut out).unwrap().unwrap();
    service.finish_data(event.message_id).unwrap();
    assert!(service.grant_replay_valid(stream, 1));
    service.done_admitted(stream).unwrap();
    assert!(!service.grant_replay_valid(stream, 1));
    assert_eq!(
        service.validate_grant(DataKind::History, stream, 1),
        Err(HistoryFailure::InvalidState)
    );
}
#[test]
fn bridge_permission_queries_do_not_conflate_scope_with_missing_stream_or_credit() {
    let access = Access(Cell::new(true));
    let lane = lane();
    let mut memory = Memory::new();
    let service = handler(&mut memory, &access, &lane);
    assert!(service.grant_authorized());
    assert!(service.dataset_authorized());
    assert_eq!(
        service.validate_grant(DataKind::History, 1, 1),
        Err(HistoryFailure::InvalidState)
    );
    access.0.set(false);
    assert!(!service.grant_authorized());
    assert!(!service.dataset_authorized());
}

#[test]
fn test_should_not_publish_read_plan_after_renewal_failure_and_allow_a_fresh_retry() {
    let access = Access(Cell::new(true));
    let lane = lane();
    let fail = Cell::new(false);
    let mut memory = Memory::new();
    memory.renew_failure = Some(&fail);
    let mut service = handler(&mut memory, &access, &lane);
    open(&mut service);
    let request = HistoryRequest::Read {
        snapshot: 1,
        after_id: 0,
        max_records: 1,
    };
    fail.set(true);
    assert_eq!(run(&mut service, request), [9, 0, 0, 0]);
    assert_eq!(service.pending_event().unwrap(), None);
    assert_eq!(service.next_data(&mut [0; 128]).unwrap(), None);
    assert_eq!(
        service.grant(DataKind::History, 1, 1),
        Err(HistoryFailure::InvalidState)
    );
    fail.set(false);
    let reply = run(&mut service, request);
    assert_eq!(&reply[..4], &[0; 4]);
    let stream = u16::from_le_bytes(reply[4..6].try_into().unwrap());
    assert_ne!(stream, 1);
    let mut body = [0; 28];
    let n = request.encode(&mut body).unwrap();
    service
        .response_admitted(frame(3, &body[..n]), &reply)
        .unwrap();
    service.grant(DataKind::History, stream, 1).unwrap();
    let event = service.next_data(&mut [0; 128]).unwrap().unwrap();
    service.finish_data(event.message_id).unwrap();
}

#[test]
fn test_should_borrow_exact_pinned_record_until_last_fragment_admission() {
    let access = Access(Cell::new(true));
    let lane = lane();
    let mut memory = Memory::new();
    let mut service = handler(&mut memory, &access, &lane);
    open(&mut service);
    let stream = read(&mut service, 0, 1);
    admit_read(&mut service, 0, 1);
    service.grant(DataKind::History, stream, 1).unwrap();
    assert_eq!(service.stage_data(41), Err(HistoryFailure::Capacity));
    let event = service.stage_data(128).unwrap().unwrap();
    assert_eq!(event.length, 42);
    let expected = service.with_data(event.message_id, <[u8]>::to_vec).unwrap();
    assert_eq!(
        &expected[..38],
        &[
            1, 0, 0, 0, 1, 0, 3, 0, 0, 0, 4, 0, 0, 0, 0, 0, 0, 0, 32, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 4, 0, 60, 0, 0, 1
        ]
    );
    assert_eq!(&expected[38..], &[0xce, 0x86, 0x0b, 0xb9]);
    let mut copied = [0; 128];
    assert_eq!(service.next_data(&mut copied).unwrap(), Some(event));
    assert_eq!(&copied[..42], &expected);
    assert!(service.with_data(event.message_id + 1, |_| ()).is_err());
    assert_eq!(service.stage_data(128).unwrap(), Some(event));
    assert_eq!(
        service.with_data(event.message_id, <[u8]>::to_vec).unwrap(),
        expected
    );
    service.finish_data(event.message_id).unwrap();
    assert!(service.with_data(event.message_id, |_| ()).is_err());
    assert_eq!(memory.committed, 0);
    assert_eq!(memory.proof, vec![4]);
}

#[test]
fn test_should_consume_exact_ready_permission_without_preparing_again() {
    let access = Access(Cell::new(true));
    let lane = lane();
    let calls = Cell::new(0);
    let mut memory = Memory::new();
    memory.prepare_calls = Some(&calls);
    let mut service = handler(&mut memory, &access, &lane);
    let mut body = [0; 28];
    let n = HistoryRequest::Count(HistoryFilter {
        domain: 32,
        filter_flags: 0,
        start_utc_ms: 0,
        end_utc_ms: 0,
        record_type: 0,
        parent_session: 0,
    })
    .encode(&mut body)
    .unwrap();
    let request = frame(7, &body[..n]);
    let mut out = [0xaa; 128];
    assert_eq!(service.response_ready(request), Ok(true));
    assert_eq!(calls.get(), 1);
    assert_eq!(service.dispatch_ready(request, &mut out), Ok(12));
    assert_eq!(
        calls.get(),
        1,
        "ready dispatch must not prepare under its response buffer stack"
    );
    assert_eq!(&out[..12], &[0, 0, 0, 0, 3, 0, 0, 0, 1, 0, 0, 0]);
    assert!(
        service.dispatch_ready(request, &mut out).is_err(),
        "a consumed readiness cannot be reused"
    );
}

#[test]
fn test_should_keep_borrowed_preparation_compatible_with_legacy_stores() {
    let access = Access(Cell::new(true));
    let calls = Cell::new(0);
    let mut memory = Memory::new();
    memory.prepare_calls = Some(&calls);
    let request = Preparation::Count {
        filter: HistoryFilter {
            domain: 32,
            filter_flags: 0,
            start_utc_ms: 0,
            end_utc_ms: 0,
            record_type: 0,
            parent_session: 0,
        },
    };
    assert_eq!(memory.prepare_ref(&request, 0, &access), Ok(true));
    assert_eq!(calls.get(), 1);
}

#[test]
fn test_should_write_the_same_read_plan_into_caller_workspace() {
    let access = Access(Cell::new(true));
    let mut memory = Memory::new();
    let filter = HistoryFilter {
        domain: 32,
        filter_flags: 0,
        start_utc_ms: 0,
        end_utc_ms: 0,
        record_type: 0,
        parent_session: 0,
    };
    let lease = memory.open(1, filter, 0, &access).unwrap();
    let expected = memory.plan(lease, 0, 16, 0, &access).unwrap();
    let mut actual = ReadPlan {
        ids: [0; 16],
        count: 0,
        end_after_id: 0,
        eof: false,
    };
    memory
        .plan_into(lease, 0, 16, 0, &access, &mut actual)
        .unwrap();
    assert_eq!(actual, expected);
    assert_eq!(actual.ids[0], 4);
    assert_eq!(actual.count, 1);
}

#[test]
fn test_should_keep_open_workspace_compatible_with_legacy_store() {
    let access = Access(Cell::new(true));
    let filter = HistoryFilter {
        domain: 32,
        filter_flags: 0,
        start_utc_ms: 0,
        end_utc_ms: 0,
        record_type: 0,
        parent_session: 0,
    };
    let mut memory = Memory::new();
    let mut output = None;
    memory
        .open_into(1, filter, 0, &access, &mut output)
        .unwrap();
    let lease = output.unwrap();
    assert_eq!(lease.snapshot.snapshot, 1);
    assert_eq!(lease.snapshot.first_id, 4);
    assert_eq!(memory.lease, Some(lease));
}

#[test]
fn test_should_validate_retained_tokens_without_scheduling_new_prefetch() {
    let access = Access(Cell::new(true));
    let lane = lane();
    let calls = Cell::new(0);
    let mut memory = Memory::new();
    memory.prefetch_calls = Some(&calls);
    let mut service = handler(&mut memory, &access, &lane);
    open(&mut service);
    let stream = read(&mut service, 0, 1);
    admit_read(&mut service, 0, 1);
    service.grant(DataKind::History, stream, 1).unwrap();
    assert!(!service.event_valid(HistoryPendingEvent::Record { stream_id: stream }, Some(1)));
    assert_eq!(
        calls.get(),
        0,
        "token validation must not admit a new backend job"
    );
    assert!(
        service
            .with_data(1, |_| panic!("no retained bytes exist"))
            .is_err()
    );
    assert_eq!(
        calls.get(),
        0,
        "a retained view must not prepare a different body"
    );
    assert!(!service.event_valid(HistoryPendingEvent::Done { stream_id: stream }, None));
    assert_eq!(calls.get(), 0);
    assert_eq!(
        service.pending_event_without_prefetch().unwrap(),
        Some(HistoryPendingEvent::Record { stream_id: stream }),
    );
    assert_eq!(
        calls.get(),
        0,
        "reservation recheck must not schedule prefetch"
    );
    let event = service.stage_data(128).unwrap().unwrap();
    assert_eq!(event.message_id, 1);
    assert!(service.event_valid(HistoryPendingEvent::Record { stream_id: stream }, Some(1)));
    assert_eq!(calls.get(), 0);
}
