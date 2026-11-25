//! Existing object wire handler over actual durable object and snapshot engines.
#![allow(clippy::unwrap_used, clippy::expect_used)]
#[path = "support/media.rs"]
mod media;
use device_object_service::{
    ObjectAccess, ObjectHandler, ObjectOperation, ObjectRepository, ObjectTransactions,
};
use device_object_store::DurableObjects;
use device_protocol::{Frame, Header, Kind, ModuleEntry, ObjectBegin, ObjectRequest, object_crc};
use device_services::ServiceHandler;
use device_storage::persistence::{AtomicSnapshot, Layout};
use device_storage::{ObjectSchema, StoreError};
use media::TestMedium;
const BODY: &[u8] = &[0, 0, 255, 255, 1, 0, 0];
#[derive(Debug, Clone, Copy)]
struct Schema;
impl ObjectSchema for Schema {
    fn validate(&self, module: u8, kind: u16, body: &[u8]) -> Result<(), StoreError> {
        if module == 4 && kind == 1 && body == BODY {
            Ok(())
        } else {
            Err(StoreError::Schema)
        }
    }
}
#[derive(Debug)]
struct Access;
impl ObjectAccess for Access {
    fn allowed(&self, _: u8, _: ObjectOperation, _: Option<u32>) -> bool {
        true
    }
    fn dangerous_allowed(&self, _: u8) -> bool {
        true
    }
}
type Repo = DurableObjects<AtomicSnapshot<TestMedium, 2048>, Schema, 2, 8, 64, 2048>;
type Handler<'a> = ObjectHandler<'a, Repo, Schema, 64>;
fn layout() -> Layout {
    Layout {
        start: 0,
        bank_bytes: 4096,
    }
}
fn repository(medium: TestMedium) -> Repo {
    DurableObjects::open(
        AtomicSnapshot::<_, 2048>::open(medium, layout()).unwrap(),
        Schema,
    )
    .unwrap()
}
fn handler<'a>(repo: &'a mut Repo, transactions: &'a ObjectTransactions<64>) -> Handler<'a> {
    ObjectHandler::new(
        ModuleEntry {
            module: 4,
            major: 1,
            minor: 0,
            availability: 1,
            features: 0,
            input_max: 512,
            output_max: 512,
        },
        repo,
        &Schema,
        &Access,
        &[1],
        transactions,
    )
    .unwrap()
}
fn run(handler: &mut Handler<'_>, request: ObjectRequest<'_>) -> Vec<u8> {
    let mut input = [0; 512];
    let n = request.encode(&mut input).unwrap();
    let mut out = [0; 512];
    let n = handler
        .dispatch(
            Frame {
                header: Header {
                    kind: Kind::Request,
                    module: 4,
                    opcode: request.opcode(),
                    message_id: 1,
                    offset: 0,
                    total_length: u16::try_from(n).unwrap(),
                },
                payload: &input[..n],
            },
            &mut out,
        )
        .unwrap();
    out[..n].to_vec()
}
fn begin(id: u32, token: u8) -> ObjectRequest<'static> {
    ObjectRequest::Begin(ObjectBegin {
        kind: 1,
        object_id: id,
        expected_revision: 0,
        total: 7,
        create: true,
        creation_token: [token; 16],
    })
}
fn transaction(response: &[u8]) -> u32 {
    assert_eq!(&response[..4], &[0; 4]);
    u32::from_le_bytes(response[4..8].try_into().unwrap())
}

#[test]
fn test_should_persist_wire_creation_and_aborted_reservation_across_reopen() {
    let mut repo = repository(TestMedium::blank());
    repo.initialize(&[4, 5]).unwrap();
    {
        let transactions = ObjectTransactions::new();
        let mut handler = handler(&mut repo, &transactions);
        let tx = transaction(&run(&mut handler, begin(10, 1)));
        assert_eq!(
            run(
                &mut handler,
                ObjectRequest::Write {
                    transaction: tx,
                    offset: 0,
                    bytes: BODY
                }
            ),
            [0, 0, 0, 0, 7, 0, 0, 0]
        );
        let reply = run(
            &mut handler,
            ObjectRequest::Commit {
                transaction: tx,
                crc: object_crc(4, 1, BODY).unwrap(),
            },
        );
        assert_eq!(&reply[..4], &[0; 4]);
        let abandoned = transaction(&run(&mut handler, begin(20, 2)));
        assert_eq!(
            run(
                &mut handler,
                ObjectRequest::Abort {
                    transaction: abandoned
                }
            ),
            [0; 4]
        );
    }
    let medium = repo.into_store().into_medium();
    let mut repo = repository(medium);
    let live = repo.id_state(4, 10, &Access).unwrap();
    assert_eq!(live.state, 1);
    assert_eq!(live.high_water, 20);
    assert_eq!(live.birth.token, [1; 16]);
    let abandoned = repo.id_state(4, 20, &Access).unwrap();
    assert_eq!(abandoned.state, 2);
    assert_eq!(abandoned.birth.token, [0; 16]);
    {
        let transactions = ObjectTransactions::new();
        let mut handler = handler(&mut repo, &transactions);
        assert_eq!(run(&mut handler, begin(19, 3)), [11, 0, 0, 0]);
        assert_eq!(run(&mut handler, begin(21, 1)), [11, 0, 0, 0]);
        let mut out = [0; 7];
        let info = handler_metadata_read(&repo, &mut out);
        assert_eq!(info, 7);
        assert_eq!(&out, BODY);
    }
    repo.delete_atomic(4, 10, 1, &Access).unwrap();
    let repo = repository(repo.into_store().into_medium());
    let tombstone = repo.id_state(4, 10, &Access).unwrap();
    assert_eq!(tombstone.state, 2);
    assert_eq!(tombstone.high_water, 20);
}
fn handler_metadata_read(repo: &Repo, out: &mut [u8]) -> usize {
    repo.read_range(4, 10, 1, 0, out, &Access).unwrap().length
}

#[test]
fn test_should_return_storage_failure_for_each_reservation_barrier_and_query_consumed_id_after_recovery()
 {
    for barrier in 1..=3 {
        let medium = TestMedium::blank();
        let control = medium.clone();
        let mut repo = repository(medium);
        repo.initialize(&[4]).unwrap();
        control.fail_sync.set(Some(control.syncs.get() + barrier));
        {
            let transactions = ObjectTransactions::new();
            let mut handler = handler(&mut repo, &transactions);
            assert_eq!(run(&mut handler, begin(5, 8)), [9, 0, 0, 0]);
        }
        assert_eq!(repo.id_state(4, 5, &Access), Err(StoreError::Backend));
        repo.recover().unwrap();
        let state = repo.id_state(4, 5, &Access).unwrap();
        assert!(matches!((state.state, state.high_water), (0, 0) | (2, 5)));
        assert_eq!(state.birth.token, [0; 16]);
        let transactions = ObjectTransactions::new();
        let mut handler = handler(&mut repo, &transactions);
        let retry = run(&mut handler, begin(6, 8));
        if state.high_water == 5 {
            assert_eq!(retry, [11, 0, 0, 0]);
        } else {
            assert_eq!(&retry[..4], &[0; 4]);
        }
    }
}
