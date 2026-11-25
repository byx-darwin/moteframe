# SYD881x runtime

Owns startup memory initialization, GPIO IRQ accumulation and the optional
single-core critical-section / Embassy polling clock. It has no board or display
dependency. The default memory/reset-sequencing code uses core only; hardware and
trouble-runtime features select the actual HAL/time adapters on ARM.

startup/ and linker/ retain each existing profile separately. Native RAM, native
Flash XIP, vendor and offline protocol-candidate boot contracts are not merged.
Application RAM initialization excludes ROM/shared RAM, NOLOAD diagnostics and
stack exactly as selected by each original linker profile.

Native lab reset preparation preserves NVIC-disable, pending-clear, SysTick-stop,
watchdog-read/mask/write order. The vendor experiment uses its separate watchdog-only
operation. Neither operation is a new product wake/reset API or a ROM vector installer.

The unsafe reset entries require exclusive initialized chip ownership. Host tests
exercise the same sequencer through a register fixture and RAM arrays, never MMIO.

The runtime clock and lab profiler share the checked polling-counter setup.
ARM LabCounter owns sampling/control MMIO; the factory comparison explicitly
selects its separate forced stop/reload/start mode. Sampling leaves COUNTFLAG
unchanged; reading control acknowledges it. Metrics and experiment decisions
remain in firmware. These adapters add no default registry dependency.

The native lab reset request also lives here: mask CPU interrupts, read the
byte-wide PWR control, clear only bit7. Firmware selects when to request it and
retains its existing delays, radio stop gates and terminal loops.
