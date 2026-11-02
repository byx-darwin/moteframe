# moteframe-storage

Read-only byte views over the existing `moteframe_core::ReadAt` contract. The
runtime is `no_std`, allocation-free, and forbids unsafe code. It adds no
upstream dependencies and performs no hidden retry.

- `SliceSource` validates that an immutable slice fits the `u32` address space,
  reads exact checked ranges, and returns the identical slice with `into_inner`.
- `ReadRef` explicitly forwards a mutable borrow, including `dyn ReadAt`, without
  adding a blanket implementation to the core trait.
- `ReadWindow` owns a source and exposes a fixed logical subrange. Construction
  checks the full range using checked addition. A rejected construction retains
  the original source in `WindowCreateError`; `error()` borrows the reason and
  `into_parts()` returns the reason and source. Formatting that error requires no
  `Debug` implementation on the source.

Each nonempty window read validates its length and local/mapped range, then
checks the source's current capacity before one exact read. Source growth never
expands the window. Source shrink can reject affected requests while leaving
surviving requests readable. Empty reads at or before the logical end succeed
without querying or reading the source, including after shrink; past-end empty
reads fail. Slice and window range failures leave the output unchanged.

`ReadError::Source` preserves the original backend error. A failed backend read
may leave partial output, which callers must discard. Providers must finish
within a finite platform-defined budget and must not retain the output buffer.
A capacity check cannot prevent an underlying resource changing before the read;
the provider remains responsible for its own validation. `ReadWindow::into_inner`
and `ReadRef::into_inner` only transfer ownership or borrowing and make no backend
calls or recovery attempts. No view retains a payload buffer or makes a snapshot
of mutable external storage.

This crate supplies reusable read views, not physical Flash, write/erase drivers,
filesystems, codecs, catalogs, or persistence logic. Real hardware adapters and
product policies belong outside this repository. Host tests establish software
behavior, not Flash timing, device operation, or whole-firmware memory use.

License: Apache-2.0; see LICENSE and NOTICE.
