# moteframe-i2c

Bounded software I²C register reads over caller-supplied open-drain `Line`
implementations and a half-cycle delay. Releasing a line must not actively drive
it high. Finite SCL polling distinguishes invalid requests, busy SDA, NACK and
clock timeout; cleanup attempts STOP and preserves the original failure.

Supports 7-bit addresses and 8-bit register pointers. The pointer write and read
START are separated by STOP, not repeated START. Payload writes, multi-master
arbitration and bus-clear recovery are not implemented. The half-cycle callback
is not a calibrated bus frequency.

Additive APIs preserve the original `Line`, `new`, `read` and exhaustive
`I2cError` contracts:

- `NormalAddress7::new` accepts ordinary targets `0x08..=0x77`; `read_normal`
  uses the legacy completion semantics. Raw `read` still accepts all seven-bit
  addresses, including special/broadcast uses with possible side effects.
- `read_checked` additionally observes actual SCL/SDA high after both the
  pointer STOP and final STOP, within bounded polling. `CheckedReadError`
  preserves protocol errors or reports `BusNotIdle`, retaining the first error.
- `try_new` returns `InitError` retaining clock/data/delay on zero budget without
  I/O. Its `into_parts` returns the error and all resources; bus `into_parts`
  returns the three resources without I/O or implicit recovery. GPIO/closure
  Debug bounds are not required by the construction error.

Choose the method explicitly; no feature flag or runtime mode is required.
The old API does not confirm physical idle. Empty reads are rejected before I/O;
Busy still attempts cleanup. Discard all output on error. Errors do not latch a
fault or retry, and callers must investigate before reuse. Line operations and
delay callbacks must themselves have finite completion budgets. Software line
observations do not qualify electrical timing, bus frequency or recovery.

See the repository guide `docs/i2c-extensibility.md` for examples, ownership
lifecycle, scope and fixed Cortex-M0 Flash/local-frame measurements.

Runtime: no_std, no allocation, unsafe forbidden. License: Apache-2.0; see LICENSE and NOTICE.
