# Portable atomic snapshot storage

`device_storage::persistence` implements the approved `Medium`, `SnapshotStore`,
and `AtomicSnapshot<M, N>` API. It is production `no_std` code with no allocation,
no third-party dependencies and `forbid(unsafe_code)`. An application injects its
own exclusive medium; this crate selects no hardware, address or Flash adapter.
Existing object/history/record APIs are unchanged.

## Usage and capacity

Choose two contiguous equal banks using `Layout { start, bank_bytes }`, then call
`AtomicSnapshot::<_, N>::open(medium, layout)`. `N` is the maximum payload length,
not an on-media address. `load` copies a whole validated snapshot from the owned
cache. A blank or incomplete initial image returns `None`; the caller must
explicitly decide whether to initialize configuration. `commit(0, bytes)` creates
revision 1 only when there is no current snapshot. Afterwards the exact current
revision is required, including for a deliberate caller-level blind write (the
caller must first obtain the current revision). Revisions never wrap.

Geometry is fixed for the medium's lifetime. Erased bytes must read as `0xff`.
Program operations change one bits to zero bits, at declared aligned units.
Reads have no alignment requirement. `write_size` must be in `1..=256`; the
256-byte upper bound permits bounded scratch space without heap allocation.
`erase_size` must be nonzero. The bank start and bank size must be multiples of
both erase and write alignment. Their checked two-bank range must fit capacity.
No power-of-two alignment assumption is made.

For write unit `w`, let `round(x) = ceil(x / w) * w`. Every bank must contain:

```
round(32) + round(N) + w <= bank_bytes
```

Construction validates these bounds and all range arithmetic before invoking
any medium operation. Revision conflicts, payload overflow and exhausted
revisions also fail before erase/program/sync. `N = 0` supports an explicitly
committed empty snapshot. A short load output returns `Capacity` without changing
its bytes or the current cache.

There is one permanent `[u8; N]` cache. Recovery also uses two temporary
`[u8; N]` arrays; programming uses a 256-byte scratch array. These are source-level
bounds, not a measured firmware stack budget. A board integration must select
`N`, audit actual stack/RAM usage and qualify its adapter.

## On-media format version 1

All multibyte integers are little endian. A bank starts with a 32-byte header:

| Offset | Length | Meaning |
| --- | --- | --- |
| 0 | 8 | ASCII `DSNAP001` format identifier |
| 8 | 8 | Nonzero `u64` revision |
| 16 | 4 | Payload byte length |
| 20 | 4 | CRC-32C of exactly the payload bytes |
| 24 | 4 | Reserved, must be zero |
| 28 | 4 | CRC-32C of header bytes 0 through 27 |

CRC is Castagnoli CRC-32C, reflected polynomial `0x82f63b78`, initial value
`0xffffffff`, final complement, using the existing `device_protocol::Crc32c`
implementation. Snapshot checksums cover raw bytes as described above; wire
object/event domain-separation prefixes are not included.
Header padding up to `round(32)` is `0xff`. Payload begins at `round(32)` and
program padding is `0xff`. The final write unit of each bank is a separate
commit marker: **every byte must equal `0xa5`**. Unused bank padding is ignored.
A committed marker with invalid header, bounds, revision or payload CRC yields
`Corrupt`, even if another valid bank exists. Equal committed revisions are
ambiguous and yield `Corrupt`, including byte-identical copies. Otherwise the
higher validated revision is selected. There is no auto-format or CRC-failure
fallback to an older/default configuration.

Only an exact marker is a committed bank. All other marker values represent
incomplete or retired storage and are ignored. This is necessary for interruption
recovery and also imposes the arbitrary-corruption limitation below.

## Commit and recovery ordering

A replacement always targets the other bank; the selected current bank is not
modified. With no current snapshot, the first bank is selected.

1. Program the target marker to all zero bytes and sync it durably. This retires
   a stale committed bank before any erase can damage its header or payload.
2. Erase the target bank; program its padded header and entire padded payload.
3. Sync all preparation durably.
4. Program only the separate final marker unit to all `0xa5` bytes.
5. Sync the marker durably, then publish the new revision and payload to RAM.

There is no fallible medium operation after the final successful sync. Every
medium error is normalized to `Io`; any attempted mutation failure leaves the
store poisoned. Subsequent `load` and `commit` return `NeedsRecovery`.
`recover` first syncs pending operations, then reads and validates both banks
before replacing the cache. A failed recovery also remains poisoned, including
when recovery was requested on a previously healthy store. `open` runs the same
sync-and-scan procedure. Opening/recovering never erases or programs storage,
although its sync may finish pending operations from an earlier failed commit.
`into_medium` transfers the medium even when recovery is required.

## Fault guarantee and its limits

The guarantee is conditional on an adapter satisfying this **logical byte-level
power-cut model**:

- A successful sync makes all preceding ordered operations durable and stable
  across restart. A failed operation/barrier may have completed none, some or
  all of its work. A later successful sync settles remaining pending operations.
- During an interrupted program or erase, each affected byte retains its state immediately before that operation or
  reaches that operation's requested complete byte; successful barriers prevent
  rollback to earlier states. Any subset
  and order of complete-byte effects is allowed. An erase byte becomes `0xff`;
  a programmed byte reaches its requested value. Commands do not affect bytes
  outside their selected range or spontaneously damage the retained bank.
- Previously durable bytes remain stable unless explicitly targeted by a later
  operation. Reads either return the actual logical bytes or report failure.
  There is one exclusive writer; external mutation behind the owned cache is
  unsupported.

Under these assumptions, retiring a stale marker ensures that a partially
erased target marker consists only of `0x00` and `0xff`, and therefore cannot
become committed. A partially programmed final marker is not committed until
all its bytes have reached `0xa5`; by then the prepared header/payload already
passed a successful durability barrier. Consequently, an interrupted commit
reopens as the complete old snapshot or complete new snapshot, or `None`/new
when there was never an initial committed image. A successfully acknowledged
commit reopens as that complete new snapshot. Errors do not publish half a
snapshot, fabricate defaults, or authorize a subsequent write without recovery.

This is **not a claim about arbitrary bit-level physical erase behavior**. A
partially erased all-zero byte can pass through `0xa5` if individual bits change
independently; the model above excludes that case. A real adapter must establish
the logical contract itself, for example through a qualified lower-level
transactional medium. CRC provides integrity detection, not authentication,
collision immunity or immunity to arbitrary corruption. In particular, damage
to an exact commit marker can make a formerly committed bank indistinguishable
from an incomplete bank; such marker damage may hide a bank. Conversely, random
or adversarial bytes can forge a marker/header/CRC. No guarantee of detecting all
arbitrary corruption is made. Intact committed-marker payload/header corruption
is explicitly tested to fail closed rather than silently select stale data.

## Offline evidence

`tests/persistence.rs` uses only test-local media, never a device adapter:

- A NOR-like medium enforces alignment and prohibits 0-to-1 programming. Every
  mutation-byte interruption point is tested in forward and reverse byte order
  through initialization and repeated bank reuse (generations 0 through 5).
  Each reopened image must be entirely old/new, and accepts a subsequent commit.
- A buffered medium keeps read-visible bytes separate from durable bytes. Across
  all three commit barriers, every prefix and suffix length from 0 through 512
  is tested as a failed partial flush (3,078 cases). Reopening never exposes an
  incomplete payload. A separate regression proves recovery syncs before
  exposing read-visible bytes as a stable recovered cache.
- Tests cover each recovery read position and failed recovery sync, cached read
  poisoning, latest/stale committed header/data corruption, equal revisions,
  CRC-valid invalid revision/length fixtures, `u64::MAX`, capacity/layout errors,
  empty/full payloads, nonzero offsets, write units 1/2/3/7/16/64/256 and redacted
  Debug output.

The initial RED log records the absent API. `red-buffered.log` records the real
behavior failure where recovery exposed an undurable new snapshot before the
recovery sync was implemented. `red-crc-reuse.log` records the independently generated
CRC-32C fixture failing before switching to the shared CRC implementation.
Focused full-crate tests and strict host/ARM
Clippy results are retained in `target/shared-config-persistence/storage/`.
These are software simulation/build results, with no hardware validation.
