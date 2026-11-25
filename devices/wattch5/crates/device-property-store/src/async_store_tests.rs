//! Private counter boundaries without adding production injection hooks.
use super::*;
use device_services::PropertyDescriptor;
use device_storage::{
    incremental::{Completion, MediumOp, StartError},
    persistence::{Geometry, Layout},
};
#[derive(Debug)]
struct Blank<'a> {
    calls: &'a Cell<usize>,
    pending: Option<Completion>,
}
impl AsyncMedium for Blank<'_> {
    fn geometry(&self) -> Geometry {
        Geometry {
            capacity: 1024,
            erase_size: 64,
            write_size: 1,
        }
    }
    fn begin(&mut self, op: MediumOp<'_>) -> Result<(), StartError> {
        self.calls.set(self.calls.get() + 1);
        self.pending = Some(match op {
            MediumOp::Read { length, .. } => Completion::Read(length),
            MediumOp::Program { .. } => Completion::Programmed,
            MediumOp::Erase { .. } => Completion::Erased,
            MediumOp::Sync => Completion::Synced,
        });
        Ok(())
    }
    fn poll(&mut self, out: &mut [u8]) -> Poll<Result<Completion, StorageError>> {
        self.calls.set(self.calls.get() + 1);
        out.fill(255);
        Poll::Ready(self.pending.take().ok_or(StorageError::Io))
    }
}
#[derive(Debug)]
struct Schema;
const D: PropertyDescriptor = PropertyDescriptor {
    key: 1,
    access: 11,
    schema: 1,
    value_max: 1,
};
const C: &[ModuleCatalogue<'_>] = &[ModuleCatalogue {
    module: 4,
    properties: &[D],
}];
impl PropertySchema for Schema {
    fn descriptor(&self, _: u8, _: u16) -> Result<PropertyDescriptor, Error> {
        Ok(D)
    }
    fn validate(&self, _: u8, _: u16, value: &[u8]) -> Result<(), Error> {
        if value == [0] {
            Ok(())
        } else {
            Err(Error::InvalidValue)
        }
    }
}
#[test]
fn test_should_reject_exhausted_owner_generation_before_engine_admission() {
    let calls = Cell::new(0);
    let mut engine = IncrementalSnapshot::<_, 128>::new_unrecovered(
        Blank {
            calls: &calls,
            pending: None,
        },
        Layout {
            start: 0,
            bank_bytes: 512,
        },
    )
    .unwrap_or_else(|e| panic!("engine {e:?}"));
    engine
        .begin_recover()
        .unwrap_or_else(|e| panic!("begin {e:?}"));
    for _ in 0..100 {
        if engine.poll_recover().is_ready() {
            break;
        }
    }
    let owner =
        AsyncPropertyStore::open(engine, C, Schema).unwrap_or_else(|e| panic!("owner {e:?}"));
    owner.inner.borrow_mut().generation = u64::MAX;
    let before = calls.get();
    assert_eq!(
        owner.begin_initialize(&[DefaultValue {
            module: 4,
            key: 1,
            value: &[0]
        }]),
        Err(Error::Storage(StorageError::RevisionExhausted))
    );
    assert_eq!(calls.get(), before);
    assert_eq!(owner.active.get(), None);
    assert_eq!(owner.revision(), Err(Error::Uninitialized));
}
