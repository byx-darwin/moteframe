//! FR8029D cold radio startup and factory calibration application.

use super::{BASEBAND, PACKET_RAM, read8, read32, write8, write32};
use crate::radio::trim::{RadioTrim, TRIM_BYTES, TrimError};

const QSPI: usize = 0xe00a_4000;
const APB0_CLOCK: usize = 0xe005_3024;
const RADIO_CLOCK: usize = 0xe005_5000;
const MODEM: usize = 0x500a_0000;
const CALIBRATION_BANK: usize = MODEM + 0x1da;

const QSPI_IDLE: u32 = 1 << 31;
const CMD_REGISTER: usize = QSPI + 0x90;
const CMD_BUSY: u32 = 3;
const JEDEC_READ: u32 = 0x9fa0_0000;
// P25Q80 Read Security Register, 3 address bytes, one dummy byte, 8 result bytes.
const SECURITY_READ: u32 = 0x48fa_0080;

pub(super) fn init<const WAIT_FOR_IDLE: bool, const MASK_IRQ: bool>() -> Result<(), u32> {
    let trim = read_trim::<WAIT_FOR_IDLE, MASK_IRQ>()?;
    enable_clocks()?;
    configure_modem(&trim);
    configure_baseband()
}

// The XIP build must keep polling inside the SRAM-resident transaction.
#[cfg_attr(feature = "radio-xip", inline(always), allow(clippy::inline_always))]
fn wait_register(address: usize, mask: u32, value: u32) -> bool {
    (0..32_768).any(|_| read32(address) & mask == value)
}

fn qspi_command<const WAIT_FOR_IDLE: bool, const MASK_IRQ: bool>(command: u32) -> Result<(), u32> {
    // The caller selects masking when its scheduler can interrupt the XIP image.
    if MASK_IRQ {
        critical_section::with(|_| qspi_transaction::<WAIT_FOR_IDLE>(command))
    } else {
        qspi_transaction::<WAIT_FOR_IDLE>(command)
    }
}

// XIP cannot fetch instructions while a QSPI software transaction is active.
#[cfg_attr(feature = "radio-xip", unsafe(link_section = ".ramfunc.qspi"))]
#[inline(never)]
fn qspi_transaction<const WAIT_FOR_IDLE: bool>(command: u32) -> Result<(), u32> {
    if !wait_register(QSPI, QSPI_IDLE, QSPI_IDLE) || !wait_register(CMD_REGISTER, CMD_BUSY, 0) {
        return Err(10);
    }
    write32(CMD_REGISTER, command);
    write32(CMD_REGISTER, command | 1);
    if !wait_register(CMD_REGISTER, CMD_BUSY, 0) {
        // On timeout the Flash caller still cannot be fetched until the command ends.
        if WAIT_FOR_IDLE {
            while read32(CMD_REGISTER) & CMD_BUSY != 0 {
                core::hint::spin_loop();
            }
        }
        return Err(10);
    }
    Ok(())
}

fn read_trim<const WAIT_FOR_IDLE: bool, const MASK_IRQ: bool>() -> Result<RadioTrim, u32> {
    qspi_command::<WAIT_FOR_IDLE, MASK_IRQ>(JEDEC_READ)?;
    if read32(QSPI + 0xa0) & 0xff_ffff != 0x14_60_85 {
        return Err(11);
    }

    let mut record = [0; TRIM_BYTES];
    for (index, pair) in record.chunks_exact_mut(8).enumerate() {
        write32(
            QSPI + 0x94,
            0x1000 + u32::try_from(index * 8).map_err(|_| 10_u32)?,
        );
        qspi_command::<WAIT_FOR_IDLE, MASK_IRQ>(SECURITY_READ)?;
        pair[..4].copy_from_slice(&read32(QSPI + 0xa0).to_le_bytes());
        pair[4..].copy_from_slice(&read32(QSPI + 0xa4).to_le_bytes());
    }
    RadioTrim::parse(&record).map_err(|error| match error {
        TrimError::Checksum => 12,
        TrimError::Version | TrimError::UnsupportedCalibration => 13,
    })
}

fn update(address: usize, mask: u32, value: u32) {
    write32(address, (read32(address) & !mask) | value);
}

fn update8(address: usize, mask: u8, value: u8) {
    write8(address, (read8(address) & !mask) | value);
}

#[inline(always)]
#[allow(clippy::inline_always)]
fn enable_clocks() -> Result<(), u32> {
    // Boot ROM leaves the MCU on 24 MHz. Select that clock for the
    // baseband and divide by two, then expose the modem and exchange RAM.
    if read32(0xe005_3004) & 3 != 0 {
        return Err(14);
    }
    let modem_apb = 1 << 10;
    update(APB0_CLOCK, modem_apb, modem_apb);
    update(RADIO_CLOCK, 0x000f_003f, 0x0001_000c);
    update(RADIO_CLOCK + 4, 1, 1);
    update(RADIO_CLOCK + 8, 0x183, 0x183);
    if !wait_register(BASEBAND + 4, u32::MAX, 0x1949_1001) {
        return Err(14);
    }
    Ok(())
}

#[inline(always)]
#[allow(clippy::inline_always)]
fn configure_modem(trim: &RadioTrim) {
    // FR8029D modem profile as observed at the packet TX interface.
    // Byte writes preserve unrelated status and control bits.
    for &(offset, value) in &[
        (0x07, 0x0f),
        (0x0a, 0x07),
        (0x0c, 0x07),
        (0x0f, 0x3f),
        (0x14, 0x3f),
        (0x3b, 0x01),
        (0x3e, 0xa0),
        (0x4f, 0x7f),
        (0x50, 0x74),
        (0xc3, 0x00),
        (0xc4, 0x02),
        (0xe0, 0x50),
        (0x192, 0x11),
        (0x193, 0x01),
        (0x2f4, 0x69),
        (0x19f, 0x11),
        (0x277, 0x00),
        (0x278, 0x00),
        (0x296, 0x04),
    ] {
        write8(MODEM + offset, value);
    }
    for &(offset, value) in &[
        (0x40, 0x8faa_9800),
        (0x44, 0x203b_0884),
        (0x48, 0x8f11_0377),
        (0x6c, 0x107f_220f),
        (0x70, 0x8080_204e),
        (0x74, 0x0e30_27e0),
        (0xcc, 0x1515_183c),
        (0xd0, 0x0600_1515),
        (0xd8, 0x110b_2f02),
        (0xdc, 0xc350_c342),
        // Analog/modem profile selected by factory trim version 2.
        (0x100, 0xbacd_cf40),
        (0x104, 0xa651_0011),
        (0x108, 0xdeff_ffb7),
        (0x10c, 0x0000_0ddd),
        (0x110, 0x3939_3737),
        (0x178, 0x5a5a_5a3c),
        (0x194, 0x2554_8e01),
        (0x198, 0x0002_10a8),
        (0x2f0, 0x6903_0708),
    ] {
        write32(MODEM + offset, value);
    }

    update8(MODEM + 0x16, 0xf0, trim.synthesizer_trim << 4);
    update8(MODEM + 0x18, 0xf0, trim.synthesizer_trim & 0xf0);
    write8(MODEM + 0x62, trim.rx_bias_trim);

    // Set the gains and load the per-unit RX and TX tables before enabling events.
    update8(CALIBRATION_BANK, 0x10, 0x10);
    update8(MODEM + 0x19c, 0x77, trim.tx_kvco_fields());

    for (index, (scale_1m, scale_2m)) in trim.tx_modulation_bands().enumerate() {
        write8(MODEM + 0x1a0 + index, trim.tx_modulation_current);
        write8(MODEM + 0x1a8 + index, trim.tx_modulation_current);
        let offset = index * 2;
        for (bank, scale) in [(0x1b0, scale_1m), (0x1c0, scale_2m)] {
            let bytes = scale.to_le_bytes();
            write8(MODEM + bank + offset, bytes[0]);
            write8(MODEM + bank + offset + 1, bytes[1]);
        }
    }
    update8(CALIBRATION_BANK, 0x10, 0);
    for (index, &dc) in trim.rx_dc.iter().enumerate() {
        write8(MODEM + 0x247 + index, dc);
    }
    update8(MODEM + 0x275, 0x80, 0x80);
    update8(MODEM + 4, 1, 1);

    update8(CALIBRATION_BANK, 0x10, 0x10);

    for (index, &step) in trim.tx_power_steps.iter().enumerate() {
        let offset = MODEM + 0x33c + 12 * index;
        let tx = u32::from_le_bytes([
            0xe0 | step,
            0x83 | (step << 2),
            0x0f | (step << 4),
            0x3e | (step >> 4),
        ]);
        write32(offset, tx);
        write32(offset + 4, tx);
        write32(offset + 8, (tx & 0xffff) | 0x000f_0000);
    }
    update8(MODEM + 0x22b, 0x20, 0);
    update(APB0_CLOCK, 1 << 10, 0);
}

#[inline(never)]
pub(super) fn configure_baseband() -> Result<(), u32> {
    const BASEBAND_RESET: u32 = 1 << 31;
    write32(BASEBAND, BASEBAND_RESET);
    if !wait_register(BASEBAND, BASEBAND_RESET, 0) {
        return Err(16);
    }
    write32(BASEBAND, 0x0010_0607);
    // Clear volatile exchange memory before the event scheduler can own it.
    for address in (PACKET_RAM..PACKET_RAM + 0x4000).step_by(4) {
        write32(address, 0);
    }
    // The controller's RF table is indexed by ascending center frequency.
    for channel in 0_u8..40 {
        write8(PACKET_RAM + 0x100 + usize::from(channel), 2 * channel);
    }
    // BLE 1M timing and the PHY interface. The free-running slot clock,
    // submit port, status and RX queue pointers are intentionally excluded.
    for &(offset, value) in &[
        (0x074, 0x0000_1020),
        (0x078, 0xc8c0_0040),
        (0x07c, 0xe442_e402),
        (0x080, 0x0060_0a60),
        (0x084, 0x0060_0a60),
        (0x088, 0x0060_0a60),
        (0x08c, 0x0000_0a4a),
        (0x090, 0x0005_0702),
        (0x094, 0x0005_0601),
        (0x098, 0x0e50_2e02),
        (0x09c, 0x0e08_0002),
        (0x130, 0xff96_02d9),
        (0x134, 0x0001_0001),
        (0x140, 0x0016_02a4),
        (0x144, 0x0000_02a4),
        (0x148, 0x10),
        (0x150, 0x13),
        (0x15c, 0x3489_adef),
        (0x160, 0x3489_afdc),
        (0x164, 0x3000_0277),
        (0x170, 0x0003_02e6),
        (0x174, 0x0302),
        (0x178, 0x0016_52fc),
        (0x17c, 0x0037_ffcb),
        (0x198, 0x0c),
        (0x1c4, 0x003f_177e),
    ] {
        write32(BASEBAND + offset, value);
    }
    // The FR8029D needs this timing word before the first scheduled event.
    // Verified by resuming the same probe on either side of the startup write.
    write32(BASEBAND + 0xe0, 0x0140_00dc);
    write32(BASEBAND + 0x20, 0x8000);
    write32(BASEBAND + 0x18, 0x8000);
    write32(BASEBAND + 0x0c, 0x0001_007e);
    write32(BASEBAND, 0x0010_0707);
    Ok(())
}
