# Immutable original-resource block archive

`device-resource-archive` is a safe `no_std`, heap-free read adapter over injected
`device_core::ReadAt`. It never performs writes or grants a physical device.
`Archive::open(reader, expected_original_sha256)` checks the complete index;
`ReadAt::read_exact` exposes the original virtual 4 MiB, independently decoding
all touched 1 KiB blocks. Every underlying read is at most 256 bytes. Busy,
recovery and I/O failures preserve the injected reader's error. Partial output
may have been copied before a later block fails, as with ordinary exact reads.
There is no persistent decoded cache that could bypass a physical reader gate.

The expected SHA comes from a separately verified candidate manifest. The
runtime compares that identity with the header, then verifies the blocks it
reads using decoded IEEE CRC32 and zlib Adler32. It does not authenticate an
archive or recompute the full SHA at mount. The host validator independently
reconstructs every byte and pins the full archive/image SHA. The caller must
supply an immutable, checked physical archive reader; original raw addresses
must never be sent directly to a physical NOR address.

## Format v1

All integers except the RFC1950 Adler32 trailer are little endian. Physical
archive extent is `[0, 0x200000)`, original virtual extent `[0, 0x400000)`.

| Header offset | Length | Value |
| --- | ---: | --- |
| 0 | 8 | `W5RARC01` |
| 8 | 2 | version 1 |
| 10 | 2 | codec 1: RFC1950 zlib / RFC1951 DEFLATE |
| 12 | 4 | original length 4194304 |
| 16 | 4 | block bytes 1024 |
| 20 | 4 | block count 4096 |
| 24 | 4 | data start 49216 (`0xc040`) |
| 28 | 4 | used end, at most 2097152 |
| 32 | 32 | complete original SHA256 |

4096 index entries follow the header, each `(physical_offset:u32,
compressed_bytes:u16, decoded_bytes:u16, decoded_crc32:u32)`. Payloads must be
canonical, contiguous and nonoverlapping from `data_start` to `used_end`, with
compressed length 6 through 1040 and decoded length exactly 1024. The host
requires all remaining archive padding to be FF. Runtime does not reread unused
padding. Index integrity is checked on mount; each accessed entry is checked
again for bounds. The physical immutable-source contract prevents remapping
entries after mount.

The decoder handles stored, fixed and dynamic Huffman blocks, including legal
multiple DEFLATE blocks, repeated code lengths, overlapping copies and an
unused absent distance tree. It rejects dictionaries, invalid/oversubscribed
or impermissibly incomplete trees, invalid repetitions, reserved symbols,
truncation, out-of-current-block distances, output overflow/short output,
unused compressed trailing bytes and Adler failures. The independent per-block
window cannot reference earlier archive blocks.

## Resource and scheduling costs

There is no heap or 32 KiB dictionary. One call holds a 1024-byte decoded
block, 256-byte physical input staging, two Huffman tables (704 bytes), and,
during dynamic table construction, 320 lengths plus a 70-byte code-length
table, along with scalar state and small offset arrays. These lifetimes
nest; they are not interchangeable scratch allowances. The `Archive` handle
adds the injected reader and one `u32`, with no resource cache.

`arm-budget.rs` monomorphizes the real decoder against a slice reader for
Thumbv6-M. The recorded Rust1.96/opt-s build gives a 12-byte handle, a
1096-byte `read_exact` frame, a 1072-byte decoder frame and a 560-byte dynamic
table frame. The maximum known local call chain from the probe is 2904 bytes.
Compiler support routines, actual physical-reader frames, the caller's output,
interrupts and complete firmware call chains remain additional/unknown; this
is not a total qualified stack bound. Evidence is generated under
`target/wattch5-integrated-candidate/task3/arm-budget/`.

Compressed input is at most 1040 bytes, or 8320 bits. Every compressed symbol
consumes at least one bit; every block consumes at least three bits; output
copies are capped at 1024 bytes; a Huffman symbol examines at most 15 bits;
all table loops have static bounds at most 320. A conservative structural bound
is below 40 million scalar loop-body executions per malicious accepted-length
block (including table setup per possible block). This is a termination bound,
not a cycle, latency or BLE fairness claim. Valid host-produced blocks are
normally one DEFLATE block. A large `read_exact` decodes as many bounded blocks
as its requested length needs and is synchronous. Callers needing foreground
fairness must limit read slices or schedule this optional archive outside the
latency-sensitive path. The active V2 UI retains its compiled font/assets and
does not need to instantiate this decoder.

## Offline reproduction

The Python tool requires explicit caller dataset, namespace, epochs, boot
provisioning identity and optional selected settings defaults. Build/verify
require the pinned duplicate original captures and an independently supplied
config. All other owner partitions stay reserved FF. An initialized region
is an offline image fact, not a service-readiness or physical-qualification
claim. The builder makes a fresh image plus untouched original restore copy,
strict manifest, embedded/full JSON archive index, all-sector change/restore
plan and SHA256SUMS. It contains no device discovery, flashing or automatic
migration/format operation.

`cargo run --offline --locked --release -p device-resource-archive --example
reconstruct -- IMAGE EXPECTED_ORIGINAL_SHA NEW_OUTPUT` exercises the real Rust
decoder over all 4096 blocks. The output must compare byte-for-byte with the
unchanged pinned capture, independently of the Python zlib implementation.
Generated test fixtures are synthetic, not vendor/resource binaries.


The generated-history interop test is deliberately explicit because its inputs
are local pinned captures and caller config: run `cargo test --offline --locked
-p device-resource-archive --test candidate_recovery -- --ignored --nocapture`.
It generates a fresh full image and recovers it through the actual broker,
IncrementalSnapshot and AsyncPagedHistoryStore(D6/R255/B474/N464). HPGMET01
binds absolute log_start0x202000 even though broker views use relative I/O.
The preserved first candidate with log_start0 is rejected; the v2 package is
checked separately. The manifest N512 is an allocation envelope; payload and
actual tested owner use464bytes.
