# moteframe-i2c

Bounded software I²C register reads over caller-supplied open-drain `Line`
implementations and a half-cycle delay. Releasing a line must not actively drive
it high. Finite SCL polling distinguishes invalid requests, busy SDA, NACK and
clock timeout; cleanup attempts STOP and preserves the original failure.

Supports 7-bit addresses and 8-bit register pointers. The pointer write and read
START are separated by STOP, not repeated START. Payload writes, multi-master
arbitration and bus-clear recovery are not implemented. The half-cycle callback
is not a calibrated bus frequency.

Runtime: no_std, no allocation, unsafe forbidden. License: Apache-2.0; see LICENSE and NOTICE.
