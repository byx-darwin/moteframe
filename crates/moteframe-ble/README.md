# moteframe-ble

Bounded, allocation-free GATT value helpers for `no_std` applications. This
crate has no runtime dependencies and forbids unsafe code. The firmware
supplies a notification backend and chooses any receive storage it needs.

## Notifications

Implement `NotificationBackend` in the platform adapter. A target must identify
the connection generation, characteristic/database, and applicable bearer.
The capacity query validates that target and its notification subscription
(CCCD), without changing the queue. Capacity includes bearer, characteristic,
and stack limits. The adapter must check target, subscription, and capacity
again when submitting because any of them can change between calls.

`CheckedNotifier::try_notify` queries capacity on every call and performs at
most one submission. It rejects advertised capacities above 512, or values
longer than the current capacity. It never truncates or retries. Zero capacity
allows an empty value if the backend accepts it. `NotifyError::Backend` retains
the original query or submission error.

`QueueAccepted` means the backend accepted/copied the whole input, allowing
immediate reuse of the input slice. It promises no radio transmission,
delivery, or peer acknowledgement. Every adapter operation must finish within
a finite platform-defined budget, including error paths. Submission errors
may leave acceptance uncertain; the helper never retries them.

Use `CheckedNotifier::new(backend)` for ownership or
`CheckedNotifier::new(NotificationRef::new(&mut backend))` for an explicit
borrow. `NotificationRef` also accepts `&mut dyn NotificationBackend` with
specified associated types. Both wrappers return their original resources
with `into_inner`, without making backend calls. No global MTU or retained
notification payload is stored.

## Complete receive writes

`WriteSlot::<Target, N>` stores one complete, already committed write. The
adapter performs any protocol assembly or commit before calling `push`.
The slot owns an actual `[u8; N]`; select `N` for application RAM needs rather
than allocating 512 bytes automatically. `N > 512` is legal, but usable payload
capacity remains `min(N, 512)`.

```rust
use moteframe_ble::{WriteBufferError, WriteMeta, WriteSlot};

fn main() -> Result<(), WriteBufferError> {
    let mut slot = WriteSlot::<u32, 8>::new();
    slot.push(42, &[1, 2, 3])?;
    let meta = WriteMeta { target: 42, length: 3 };
    assert_eq!(slot.peek(), Some((meta, &[1, 2, 3][..])));
    let mut output = [0; 8];
    assert_eq!(slot.take_into(&mut output)?, Some(meta));
    assert_eq!(&output[..3], &[1, 2, 3]);
    Ok(())
}
```

`push` first rejects an occupied slot, then rejects excessive length, leaving
existing data and identity unchanged. `peek` exposes only the valid prefix.
`take_into` copies the complete event and clears it; a too-small output leaves
both the slot and destination unchanged, and output tails are untouched.
`clear` explicitly discards a pending event and returns its original metadata.
Clearing/taking does not zero storage. Old connection events are never relabeled.
Even `N = 0` can retain a real empty event, distinct from no event.

The slot provides no queue, heap, background work, or concurrency guarantee.
Firmware owns synchronization and decides whether to discard stale events.

The 512-byte attribute value bound follows
[Bluetooth Core 5.4 ATT, section 3.2.9](https://www.bluetooth.com/wp-content/uploads/Files/Specification/HTML/Core-54/out/en/host/attribute-protocol--att-.html).
A notification's actual allowed value can be smaller, so firmware must report
the current adapter capacity rather than infer a universal MTU here. GAP,
pairing, indications, prepared-write assembly, radio implementations, and
product protocols remain outside this crate. Host tests establish software
contract behavior only, not hardware or interoperability qualification.
