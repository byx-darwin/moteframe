#!/usr/bin/env python3
"""Package the FR8029D Rust startup probe as reversible offline sector files."""
import argparse
from dataclasses import dataclass
from hashlib import sha256
import json
from pathlib import Path
import struct
from zlib import crc32

FLASH_BASE = 0x10000000
FLASH_SIZE = 0x100000
SECTOR_BYTES = 0x1000
APP_OFFSET = 0x2000
APP_ADDRESS = FLASH_BASE + APP_OFFSET
STACK_TOP = 0x20008000
VECTOR_BYTES = 320
MAGIC = 0x51525251


@dataclass(frozen=True)
class SectorPatch:
    """Replacement and recovery data for one physical Flash sector."""

    offset: int
    original: bytes
    replacement: bytes


def validate_image(flash):
    """Check the observed primary boot header, both CRCs, and the initial vectors."""
    if len(flash) != FLASH_SIZE:
        raise ValueError("Expected a complete 1 MiB Flash XIP capture")
    version, store, size, execute, unit, step, checksum, valid, options, info, tag, policy = (
        struct.unpack_from("<12I", flash)
    )
    if (version, store, execute, unit, step, valid, options, tag, policy) != (
        0xFFFFFFFF, 0, APP_OFFSET, 0x4000, 0x20, MAGIC, 0x00D02E83, MAGIC, 0x8020
    ):
        raise ValueError("Header does not match the captured FR8029D boot policy")
    if crc32(flash[:36]) != info:
        raise ValueError("Boot header CRC mismatch")
    if not 8 <= size <= FLASH_SIZE - APP_OFFSET:
        raise ValueError("Boot payload size outside the Flash image")
    if crc32(flash[APP_OFFSET:APP_OFFSET + size]) != checksum:
        raise ValueError("Application payload CRC mismatch")
    stack, reset = struct.unpack_from("<II", flash, APP_OFFSET)
    if stack % 8 or not 0x20003000 < stack <= 0x20018000:
        raise ValueError("Unexpected application initial stack")
    if not reset & 1 or not APP_ADDRESS + 8 <= (reset & ~1) <= APP_ADDRESS + size - 2:
        raise ValueError("Initial Thumb vector outside the Flash payload")
    return {
        "payload_offset": APP_OFFSET,
        "payload_bytes": size,
        "payload_crc32": f"{checksum:08x}",
        "header_crc32": f"{info:08x}",
        "initial_msp": hex(stack),
        "reset_vector": hex(reset),
    }


def extract_probe(elf):
    """Extract a bounded XIP image and any contiguous RAM initializers."""
    if len(elf) < 52 or elf[:7] != b"\x7fELF\x01\x01\x01":
        raise ValueError("Expected a little-endian ELF32 probe")
    kind, machine, version, entry, offset, _, _, hsize, entsize, count = (
        struct.unpack_from("<HHIIIIIHHH", elf, 16)
    )
    if ((kind, machine, version, hsize, entsize) != (2, 40, 1, 52, 32)
            or offset < hsize or offset + entsize * count > len(elf)):
        raise ValueError("Invalid ELF32 ARM executable program table")
    headers = (struct.unpack_from("<8I", elf, offset + i * entsize) for i in range(count))
    loads = [header for header in headers if header[0] == 1]
    if not 1 <= len(loads) <= 2:
        raise ValueError("Expected one load segment and at most one RAM initializer")
    _, data, virtual, physical, size, memory, flags, alignment = loads[0]
    if (virtual, physical, flags, alignment) != (APP_ADDRESS, APP_ADDRESS, 5, 512):
        raise ValueError("Probe segment does not have the expected executable XIP mapping")
    if (not VECTOR_BYTES + 2 <= size <= SECTOR_BYTES or size != memory
            or data < offset + entsize * count or data + size > len(elf)
            or data % alignment or size % 4):
        raise ValueError("Truncated, unaligned or oversized probe segment")
    payload = elf[data:data + size]
    stack, reset = struct.unpack_from("<II", payload)
    if stack != STACK_TOP:
        raise ValueError("Probe initial stack does not match its reserved SRAM")
    if not reset & 1 or not APP_ADDRESS + VECTOR_BYTES <= (reset & ~1) <= APP_ADDRESS + size - 2:
        raise ValueError("Probe needs a Thumb reset vector inside its executable code")
    if entry != reset:
        raise ValueError("ELF entry does not match the probe reset vector")
    if len(loads) == 2:
        _, ram_data, ram_virtual, ram_physical, ram_size, ram_memory, ram_flags, ram_align = loads[1]
        if (ram_virtual != 0x20003000 or ram_physical != APP_ADDRESS + size
                or not 0 < ram_size == ram_memory <= 0x3F00
                or size + ram_size > SECTOR_BYTES
                or ram_flags != 6 or ram_align != 4
                or ram_data < data + size or ram_data + ram_size > len(elf)
                or ram_data % 4 or ram_size % 4):
            raise ValueError("Expected one load segment and only a contiguous Flash-backed RAM initializer")
        payload += elf[ram_data:ram_data + ram_size]
    return payload


def build_package(stock, elf):
    """Return the verified offline Flash overlay and the two reversible patches."""
    validate_image(stock)
    if stock[SECTOR_BYTES:APP_OFFSET] != bytes(SECTOR_BYTES):
        raise ValueError("The OTA metadata sector differs from the empty sector in the capture")
    payload = extract_probe(elf)
    header = bytearray(stock[:SECTOR_BYTES])
    # Reserve the end of the first boot-header page, outside the ROM fields.
    marker = b"MF01" + sha256(stock).digest()
    if header[220:256] != b"\xff" * len(marker):
        raise ValueError("Backup marker area is occupied")
    header[220:256] = marker
    struct.pack_into("<I", header, 8, len(payload))
    struct.pack_into("<I", header, 24, crc32(payload))
    struct.pack_into("<I", header, 36, crc32(header[:36]))
    app = payload.ljust(SECTOR_BYTES, b"\xff")
    patches = (
        SectorPatch(APP_OFFSET, stock[APP_OFFSET:APP_OFFSET + SECTOR_BYTES], app),
        SectorPatch(0, stock[:SECTOR_BYTES], bytes(header)),
    )
    overlay = bytearray(stock)
    for patch in patches:
        overlay[patch.offset:patch.offset + SECTOR_BYTES] = patch.replacement
    validate_image(overlay)
    return bytes(overlay), patches


def write_package(output, stock, elf):
    """Write local sector files and a manifest after validating the rollback overlay."""
    overlay, patches = build_package(stock, elf)
    restored = bytearray(overlay)
    for patch in patches:
        restored[patch.offset:patch.offset + SECTOR_BYTES] = patch.original
    if restored != stock:
        raise ValueError("Sector restoration did not reproduce the original Flash XIP capture")

    report = {
        "scope": "Offline verification only. Board write and boot have not been verified.",
        "original_sha256": sha256(stock).hexdigest(),
        "candidate_overlay_sha256": sha256(overlay).hexdigest(),
        "rollback_overlay_sha256": sha256(restored).hexdigest(),
        "probe_elf_sha256": sha256(elf).hexdigest(),
        "original_image": validate_image(stock),
        "candidate_image": validate_image(overlay),
        "ota_sector_unchanged": True,
        "sectors": [],
    }
    output.mkdir(mode=0o700)
    for patch in patches:
        prefix = f"sector-{patch.offset:05x}"
        entry = {"offset": hex(patch.offset), "xip_address": hex(FLASH_BASE + patch.offset),
                 "bytes": SECTOR_BYTES}
        for role, data in (("probe", patch.replacement), ("restore", patch.original)):
            name = f"{prefix}-{role}.bin"
            (output / name).write_bytes(data)
            entry[role] = {"file": name, "sha256": sha256(data).hexdigest()}
        report["sectors"].append(entry)
    (output / "manifest.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--original", type=Path, required=True)
    parser.add_argument("--repeat-capture", type=Path, required=True)
    parser.add_argument("--elf", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True, help="new private output directory")
    args = parser.parse_args()
    try:
        if args.original.samefile(args.repeat_capture):
            raise ValueError("Two separate board captures are required")
        if any(path.stat().st_size != FLASH_SIZE for path in (args.original, args.repeat_capture)):
            raise ValueError("Both captures must contain the complete 1 MiB XIP window")
        stock = args.original.read_bytes()
        if stock != args.repeat_capture.read_bytes():
            raise ValueError("The two Flash captures differ")
        write_package(args.output, stock, args.elf.read_bytes())
    except (OSError, ValueError) as error:
        parser.error(str(error))
    print(args.output / "manifest.json")


if __name__ == "__main__":
    main()
