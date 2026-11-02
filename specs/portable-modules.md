# Portable storage, BLE and application memory profiles

Approved scope: the user requested multi-role extensibility/compatibility assessment followed by autonomous coding, README RAM guidance, merge and remote push on 2026-11-03. The three role assessments selected the bounded first release below. Preserve all existing core/I²C/display/UI changes and validate them together; no hardware, vendor or private product migration.

## Architecture and compatibility

- Add independent `moteframe-storage` and `moteframe-ble` crates. Runtime remains no_std, allocation-free and unsafe forbidden. No new upstream dependencies; storage depends only on existing core::ReadAt, BLE has no runtime dependencies. Firmware chooses its crate dependencies and buffer sizes; core/UI never depend on BLE/storage.
- Do not modify existing core/UI/I²C trait requirements, exhaustive errors, default features or object fields. Use explicit reference wrappers to support borrowed/dyn providers without introducing blanket impls on old traits.
- New extension errors are non_exhaustive. Preserve raw backend failures, finite-operation obligations and no hidden retry. Resource extraction only transfers ownership.
- Initial storage is read-only reusable views; physical Flash/read/write/erase drivers, filesystems, codecs, catalogs and persistence business remain external. Initial BLE is portable value handling, not a host/controller/radio implementation, GAP discovery, pairing, indications, prepared-write assembly or product message protocol.

## Storage public contract

`RangeError::{LengthTooLarge,OutOfBounds}`; `ReadError<E>::{Range(RangeError),Source(E)}`.

`SliceSource<'a>::new(&'a[u8])->Result<Self,RangeError>` validates representable u32 capacity; ReadAt reads exact bytes and validates range before touching output; into_inner returns the identical slice.

`ReadRef<'a,R:?Sized>::new(&'a mut R)` / into_inner explicitly forwards ReadAt, including dyn sources. No blanket core impl is added.

`ReadWindow<R:ReadAt>::new(inner,start:u32,length:u32)->Result<Self,WindowCreateError<R>>` checks the full initial range using checked addition. Failure retains the original non-Copy/non-Debug source; error()/into_parts expose reason and resource. Successful window capacity is fixed logical length. Each nonempty read checks length conversion, local and mapped range, then current source capacity before exactly one delegation. Valid empty reads within the logical window succeed without backend read even after source shrink; past-end empties fail. Source growth never enlarges window; shrink can reject affected nonempty requests without backend read. Errors may leave partial output; never retry. into_inner performs no transfer. Public docs/Debug required, construction-error Debug omits backend.

## BLE public contract

`NotificationBackend` has associated `Target:Copy+Eq` and `Error`, `notification_capacity(&self,target)->Result<usize,Error>`, `try_queue_notification(&mut self,target,value:&[u8])->Result<(),Error>`. Target identity must include connection generation, characteristic/database identity and applicable bearer. Capacity query validates the current target/CCCD and has no queue mutation. The backend must revalidate target/capacity at submission against races.

`CheckedNotifier<B>::new(B)` / into_inner have no backend calls. `try_notify` queries every call, rejects capacity>512 or value length>current capacity before submission; zero capacity may accept empty values if the backend permits them. Submit at most once. Return `QueueAccepted`, meaning only input copied/accepted by backend and immediately reusable, never over-air or peer acknowledgement. `NotifyError<E>::{Backend(E),InvalidCapacity{capacity},TooLong{length,capacity}}` retains raw errors. Explicit `NotificationRef<'a,B:?Sized>` supports borrowed/dyn backends. No retained payload buffer or cached global MTU.

`WriteMeta<T>{target:T,length:usize}` and `WriteSlot<T:Copy,const N:usize>` store one complete, already committed receive write. push first rejects Full, then lengths exceeding min(N,512), and otherwise copies the whole value. No truncation or vendor receive operations. peek exposes only the valid prefix. take_into returns None for empty, or copies the full event and clears pending; too-small destination leaves slot and destination unchanged; tail untouched. clear explicitly discards and returns metadata, never relabels old connection events. N=0 supports a real empty event distinct from no event. N>512 is allowed as an allocation choice but usable payload remains<=512. No default 512-byte instance, heap, queue, background work or concurrency guarantee.

ATT attribute limits/notification semantics are checked against [Bluetooth Core 5.4 ATT](https://www.bluetooth.com/wp-content/uploads/Files/Specification/HTML/Core-54/out/en/host/attribute-protocol--att-.html); adapter capacities include bearer, characteristic and stack limits, rather than this crate deriving a universal MTU.

## Maintained application RAM example

Standalone `examples/ui-low-memory/` owns `ui-low-memory=[]` with default empty feature set; library feature/defaults stay unchanged. One UI_PIXELS constant selects 8/32 for both actual scratch and StreamTarget RUN. Typed stream/config/session errors remain distinct. Host tests and ARM build run separately in both modes; verify final image, actual array capacity, 16/64 refresh/service calls for 64x8, 2/8 stream writes for 64 pixels, exact/oversized Fixed limits, display/service failures and retry, idle and deferred invalidation. The example scene is Infallible; failing resource/scene retention and retry remain exercised by the maintained UI session tests rather than adding test-only failure controls to the sample. Root README links a runnable relative-path example, explains workspace buffers vs local frame and feature-unification behavior, and labels old profile numbers as fixed offline measurements.

## Evidence and integration

Keep both existing frozen core/I²C JSON references, their measurements and historical source digests unchanged. Split immutable baseline manifest provenance from current six-crate manifests. Permit only an explicitly reviewed exact Cargo-context migration (original reference hash plus exact before/after Cargo lock+manifest hashes). Compiler/profile/linker/probe/nontransport configuration must stay identical; unrecorded changes and positive legacy resource deltas still fail. Fix context(root) to read root's lockfile. Add migration rejection tests. Do not silently refresh baselines.

Measure fixed Cortex-M0 consumers for new modules, including 8/32-byte BLE buffers, owned/borrowed storage and notifications. Record object layout, linked Flash, static data/bss and local frames; no whole-firmware/hardware claims. Keep new measurement probes outside packages and validate public source/package/supply-chain boundaries.

Full make check build, supply-chain and required make hooks must pass before authorized commits. Fetch/reconcile main, merge without force or history rewriting, verify merged result and push origin/main (and feature branch for traceability). Observe remote CI where available. Never publish a release or operate hardware.
