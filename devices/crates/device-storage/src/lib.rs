//! Media-independent storage protocol engines.
#![no_std]
mod object;
pub use object::{
    Authorization, Begin, Commit, ObjectEngine, ObjectMeta, ObjectSchema, ObjectStore, StoreError,
};
mod history;
pub use history::{DataIds, DataStream, HistoryError, Snapshot};
mod record;
pub use record::{CommitState, RecordIntegrity, StoredRecord};
pub mod incremental;
pub mod partitions;
pub mod persistence;
