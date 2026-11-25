# SYD881x HAL

Typed chip registers and exclusive peripheral ownership for the existing verified
mapping. GPIO, SPI, PWM, open-drain pins and cycle arithmetic live here; product
slave-select pins, panel commands and touch layouts do not.

SPI and PWM reserve both their selected pins and the physical controller. Clean
release detaches muxes into input state; SPI timeout retains its controller and
pins for explicit hardware recovery. Supplying a different pin set cannot bypass
that fault. Callers must supply verified mux-capable pins for their actual board.

The ARM-only hardware module owns volatile accesses and singleton runtime tokens.
RawRadioMmio is an unsafe, stateless transport for the existing exclusive reset
experiments, with no hidden assumption about initialized singleton flags. It does
not initialize clocks, perform RF calibration or prove hardware readiness.

Existing board_syd8810 paths delegate/reexport these types and operations.
