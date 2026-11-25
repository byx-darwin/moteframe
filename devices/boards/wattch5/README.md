# Wattch5 board and compatibility facade

Package name remains board-syd8810. This directory owns actual pin connections,
selected panel/touch/resource definitions and shared-SPI slave deselection. Typed
register algorithms and MMIO are reexported from syd881x-hal; IRQ/time integration
is reexported from syd881x-runtime. Panel commands/reset and bus protocols live in
reusable drivers.

| Connection | Existing pins |
| --- | --- |
| Display MOSI / clock / CS / DC / reset / backlight | 14 / 19 / 10 / 9 / 1 / 6 |
| Shared SPI MISO / other slave CS | 12 / 15,16 |
| Touch reset / IRQ / clock / data / side key | 23 / 22 / 20 / 21 / 25 |

The old drivers::Board constructor/methods, handle/error types, clock/register
modules, panel constants and ARM hardware entry names remain available. The board
wrapper supplies its explicit pin configuration to the HAL. No register algorithm,
IRQ implementation or panel command table is duplicated here.

Controller address0x15 and read registers1,4,6,2 preserve the existing input path;
no touch updater or business gesture policy is introduced. The selected panel
profile is the pinned original128x128 sequence, not a claim about exact silicon ID.
