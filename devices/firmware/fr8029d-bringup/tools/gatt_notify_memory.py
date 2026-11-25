#!/usr/bin/env python3
"""Compare FR8029D Notify image footprints or prepare an SRAM-only trial."""
import argparse
from dataclasses import dataclass
from hashlib import sha256
import json
from pathlib import Path
import re
import struct


APP_SRAM_START = 0x20003000
SRAM_END = 0x20018000
FLASH_APP_START = 0x10002000
FLASH_END = 0x10100000
VECTOR_BYTES = 320
DIAGNOSTIC_BYTES = 256
ELF_HEADER = struct.Struct("<16sHHIIIIIHHHHHH")
PROGRAM = struct.Struct("<8I")
SECTION = struct.Struct("<10I")
SYMBOL = struct.Struct("<IIIBBH")
BARE_IDENTITIES = {False: b"FR29-GAT2", True: b"FR29-XN01"}
RTOS_IDENTITY = b"FR29-RTOS"


@dataclass(frozen=True)
class Image:
    digest: str
    identity: str
    entry: int
    vector: int
    loads: list
    data_bytes: int
    bss_bytes: int
    data_bss_alignment_bytes: int
    static_bytes: int
    diagnostics: tuple
    stacks: dict
    task_stack_padding_bytes: int
    runtime_words: dict
    event_driven: bool

    @property
    def payload_bytes(self):
        return sum(len(data) for _, data in self.loads)

    def report(self):
        stacks = {label: size for label, (_, size) in self.stacks.items()}
        reserved = sum(stacks.values()) + self.task_stack_padding_bytes
        return {
            "elf_sha256": self.digest,
            "identity": self.identity,
            "event_driven": self.event_driven,
            "image_payload_bytes": self.payload_bytes,
            "data_bytes": self.data_bytes,
            "bss_bytes": self.bss_bytes,
            "data_bss_alignment_bytes": self.data_bss_alignment_bytes,
            "static_bytes": self.static_bytes,
            "diagnostics_bytes": self.diagnostics[1],
            "stack_reserved_bytes": stacks,
            "stack_reserved_total_bytes": reserved,
            "task_stack_padding_bytes": self.task_stack_padding_bytes,
            "total_reserved_sram_bytes": self.static_bytes + self.diagnostics[1] + reserved,
        }


def elf_rows(blob, offset, row_size, count, layout):
    if row_size != layout.size or offset < ELF_HEADER.size or offset + row_size * count > len(blob):
        raise ValueError("Missing or truncated ELF table")
    return list(layout.iter_unpack(blob[offset:offset + row_size * count]))


def elf_sections_and_symbols(blob, header):
    sections = elf_rows(blob, header[6], header[11], header[12], SECTION)

    def table(index):
        if not 0 < index < len(sections):
            raise ValueError("Invalid ELF string table index")
        section = sections[index]
        offset, size = section[4:6]
        if section[1] != 3 or offset + size > len(blob):
            raise ValueError("Invalid ELF string table")
        return blob[offset:offset + size]

    def name(strings, offset):
        end = strings.find(b"\x00", offset)
        if end < 0:
            raise ValueError("Unterminated ELF name")
        return strings[offset:end].decode("ascii", errors="replace")

    names = table(header[13])
    by_name = {name(names, section[0]): section for section in sections}
    symtab = by_name.get(".symtab")
    if symtab is None or symtab[9] != SYMBOL.size or symtab[5] % SYMBOL.size:
        raise ValueError("Expected an unstripped ELF symbol table")
    strings = table(symtab[6])
    symbols = {
        name(strings, name_offset): (value, size, index)
        for name_offset, value, size, _, _, index in elf_rows(
            blob, symtab[4], symtab[9], symtab[5] // SYMBOL.size, SYMBOL
        )
        if index != 0
    }
    return sections, by_name, symbols


def inspect_image(blob, *, xip):
    if len(blob) < ELF_HEADER.size:
        raise ValueError("Truncated ELF header")
    header = ELF_HEADER.unpack_from(blob)
    if header[0][:7] != b"\x7fELF\x01\x01\x01" or header[1:4] != (2, 40, 1) or header[8] != 52:
        raise ValueError("Expected a little-endian ELF32 ARM executable")
    programs = elf_rows(blob, header[5], header[9], header[10], PROGRAM)
    sections, by_name, symbols = elf_sections_and_symbols(blob, header)

    def address(name):
        try:
            return symbols[name][0]
        except KeyError as error:
            raise ValueError(f"Missing linker symbol {name}") from error

    vector = address("__vector_table")
    image_limit = FLASH_END if xip else SRAM_END
    valid_origin = vector == FLASH_APP_START if xip else APP_SRAM_START <= vector < SRAM_END
    if not valid_origin or vector % 512:
        raise ValueError(f"Expected an aligned {'Flash/XIP' if xip else 'SRAM'} image vector table")
    loads = []
    end = vector
    for _, offset, virtual, physical, size, memory, flags, _ in sorted(
        (row for row in programs if row[0] == 1 and row[4]), key=lambda row: row[3]
    ):
        if physical != end or physical + size > image_limit:
            raise ValueError("Expected contiguous PT_LOAD payload within the image window")
        if size % 4 or size > memory or offset + size > len(blob):
            raise ValueError("Invalid PT_LOAD file bytes")
        if not loads:
            if (virtual, flags) != (vector, 5) or size < VECTOR_BYTES + 2:
                raise ValueError("Expected executable vector and code load")
        elif flags != 6 or not APP_SRAM_START <= virtual < virtual + memory <= SRAM_END:
            raise ValueError("Expected image-backed SRAM data initializer")
        loads.append((physical, blob[offset:offset + size]))
        end += size
    if not loads:
        raise ValueError("Missing image load segments")

    msp, entry = struct.unpack_from("<II", loads[0][1])
    bottom = address("__stack_bottom")
    if (entry != header[4] or not entry & 1
            or not vector + VECTOR_BYTES <= entry - 1 < vector + len(loads[0][1])):
        raise ValueError("The ELF entry must match the Thumb reset vector inside code")
    if msp != address("__stack_top") or msp % 8 or bottom % 8 or bottom >= msp:
        raise ValueError("Invalid initial MSP or linker stack reservation")

    wake_vectors = {0: "fr29_baseband_irq", 16: "fr29_timer_irq"}
    event_driven = "__gatt_event_wake" in symbols
    if not event_driven and any(name in symbols for name in wake_vectors.values()):
        raise ValueError("Wake handlers require the event-driven linker layout")
    if event_driven:
        for irq, name in wake_vectors.items():
            handler = address(name) | 1
            actual, = struct.unpack_from("<I", loads[0][1], (16 + irq) * 4)
            if actual != handler or not vector + VECTOR_BYTES <= handler - 1 < vector + len(loads[0][1]):
                raise ValueError(f"Invalid event wake vector for IRQ {irq}")

    diag = by_name.get(".diagnostics")
    if (diag is None or diag[1] != 8 or diag[3] != address("__diagnostics_start")
            or diag[5] != DIAGNOSTIC_BYTES or diag[3] % 4):
        raise ValueError("Invalid diagnostic reservation")
    sdata, edata, sbss, ebss = (address(name) for name in ("__sdata", "__edata", "__sbss", "__ebss"))
    if not APP_SRAM_START <= sdata <= edata <= sbss <= ebss <= SRAM_END:
        raise ValueError("Invalid .data and .bss bounds")
    ranges = [(sdata, ebss, "static"), (diag[3], diag[3] + diag[5], "diagnostics"),
              (bottom, msp, "msp")]

    stacks = {"msp": (bottom, msp - bottom)}
    stack_section = by_name.get(".freertos_stacks")
    task_stack_padding = 0
    if stack_section is not None:
        stack_base, stack_length = stack_section[3], stack_section[5]
        if stack_section[1] != 8 or stack_section[2] & 3 != 3:
            raise ValueError("FreeRTOS stacks must be allocated writable NOLOAD storage")
        ranges.append((stack_base, stack_base + stack_length, "task stacks"))
        for label in ("ble", "idle"):
            task = symbols.get(f"fr29_{label}_task_stack")
            if (task is None or not 0 < task[2] < len(sections)
                    or sections[task[2]] != stack_section):
                raise ValueError(f"Missing FreeRTOS {label} stack symbol in its stack section")
            base, size, _ = task
            if (base % 8 or size % 4
                    or not stack_base <= base < base + size <= stack_base + stack_length):
                raise ValueError(f"Invalid FreeRTOS {label} task stack extent")
            stacks[label] = (base, size)
        first, second = sorted((stacks["ble"], stacks["idle"]))
        if first[0] + first[1] > second[0]:
            raise ValueError("Overlapping FreeRTOS task stacks")
        task_stack_padding = stack_length - first[1] - second[1]

    executable = b"".join(data for _, data in loads)
    identity = RTOS_IDENTITY if stack_section is not None else BARE_IDENTITIES[xip]
    if identity not in executable:
        raise ValueError(f"Missing {identity.decode()} identity in image payload")
    if not xip:
        ranges.append((vector, end, "SRAM image"))
    for start, stop, label in ranges:
        if not APP_SRAM_START <= start <= stop <= SRAM_END:
            raise ValueError(f"{label} is outside the trial SRAM window")
    occupied = sorted(ranges)
    for (_, stop, label), (start, _, next_label) in zip(occupied, occupied[1:]):
        if stop > start:
            raise ValueError(f"{label} overlaps {next_label}")

    for section in sections:
        start, size = section[3], section[5]
        if section[2] & 2 and APP_SRAM_START <= start < SRAM_END and size:
            if not any(lo <= start < start + size <= hi for lo, hi, _ in ranges):
                raise ValueError(f"Unaccounted allocated SRAM section at {start:#x}")

    runtime_words = {}
    for name, label in (("fr29_freertos_status", "status"), ("xTickCount", "tick")):
        symbol = symbols.get(name)
        if symbol is not None:
            value, size, _ = symbol
            if size != 4 or value % 4 or not sdata <= value < value + size <= ebss:
                raise ValueError(f"Invalid {name} runtime word")
            runtime_words[label] = value

    return Image(
        digest=sha256(blob).hexdigest(), identity=identity.decode(), entry=entry - 1,
        vector=vector, loads=loads, data_bytes=edata - sdata, bss_bytes=ebss - sbss,
        data_bss_alignment_bytes=sbss - edata, static_bytes=ebss - sdata,
        diagnostics=(diag[3], diag[5]), stacks=stacks, task_stack_padding_bytes=task_stack_padding,
        runtime_words=runtime_words,
        event_driven=event_driven,
    )


def render_trial(image):
    lines = [
        "# Generated from the exact SRAM ELF. Source before gatt_notify_sram_trial.tcl.",
        f"set notify_elf_sha256 {image.digest}",
        f"set notify_identity {image.identity}",
        f"set notify_vector {image.vector:#x}",
        f"set notify_entry {image.entry:#x}",
        f"set notify_diag {image.diagnostics[0]:#x}",
        f"set notify_event_driven {int(image.event_driven)}",
        "set notify_runtime_words {" + " ".join(
            f"{label} {value:#x}" for label, value in image.runtime_words.items()
        ) + "}",
        "set notify_stacks {",
    ]
    for label, (base, size) in image.stacks.items():
        paint = 0xa55a3cc3 if label == "msp" else 0xa5a5a5a5
        lines.append(f"    {{{label} {base:#x} {size} {paint:#x}}}")
    lines += ["}", "set notify_regions {"]
    for address, data in image.loads:
        words = " ".join(f"{word:#x}" for word, in struct.iter_unpack("<I", data))
        lines.append(f"    {{{address:#x} {{{words}}}}}")
    lines += ["}", "set notify_image_ready 1", ""]
    return "\n".join(lines)


def observe_log(path, image):
    blob = path.read_text()
    digests = re.findall(r"^NOTIFY_TRIAL [^ ]+ ELF_SHA256=([0-9a-f]{64})$", blob, re.M)
    points = re.findall(r"^STACK_WATERMARK (msp|ble|idle) RESERVE=(\d+) TOUCHED=(\d+)$", blob, re.M)
    if digests != [image.digest] or len(points) != len(image.stacks):
        raise ValueError(f"{path}: missing or mismatched SRAM trial ELF/stack measurements")
    observations = {}
    for label, reserve, touched in points:
        reserve, touched = int(reserve), int(touched)
        if label in observations or image.stacks.get(label, (0, -1))[1] != reserve or touched > reserve:
            raise ValueError(f"{path}: invalid {label} reservation or watermark")
        observations[label] = touched
    return {
        "last_session_stack_watermark_bytes": observations,
        "board_notification_observed": bool(re.search(r"^PHONE_NOTIFICATION_VERIFIED ", blob, re.M)),
        "tx_queue_drained": bool(re.search(r"^TX_QUEUE_CONFIRMED ", blob, re.M)),
        "stock_prefix_verified": "STOCK_RESTORED FLASH_PREFIX_BYTES=12288" in blob,
    }


def compare(args):
    report = {}
    for variant in ("bare", "rtos"):
        xip_path = getattr(args, f"{variant}_xip")
        sram_path = getattr(args, f"{variant}_sram")
        log_path = getattr(args, f"{variant}_log")
        identity = (RTOS_IDENTITY if variant == "rtos" else BARE_IDENTITIES[True]).decode()
        xip = inspect_image(xip_path.read_bytes(), xip=True)
        if xip.identity != identity:
            raise ValueError(f"Expected a {identity} XIP ELF")
        report[variant] = {"flash_xip": xip.report()}
        if log_path and not sram_path:
            raise ValueError(f"--{variant}-log requires --{variant}-sram for the exact tested ELF")
        if sram_path:
            sram = inspect_image(sram_path.read_bytes(), xip=False)
            sram_identity = (RTOS_IDENTITY if variant == "rtos" else BARE_IDENTITIES[False]).decode()
            if sram.identity != sram_identity:
                raise ValueError(f"Expected a {sram_identity} SRAM ELF")
            trial = sram.report()
            trial["total_occupied_sram_bytes"] = trial["total_reserved_sram_bytes"] + sram.payload_bytes
            report[variant]["sram_trial"] = trial
            if log_path:
                report[variant]["sram_trial"].update(observe_log(log_path, sram))
    comparison_keys = (
        "image_payload_bytes", "static_bytes", "stack_reserved_total_bytes", "total_reserved_sram_bytes",
    )
    for mode in ("flash_xip", "sram_trial"):
        if all(mode in report[variant] for variant in ("bare", "rtos")):
            bare, rtos = (report[variant][mode] for variant in ("bare", "rtos"))
            keys = comparison_keys
            if mode == "sram_trial":
                keys += ("total_occupied_sram_bytes",)
            report[f"rtos_minus_bare_{mode}_bytes"] = {key: rtos[key] - bare[key] for key in keys}
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    subcommands = parser.add_subparsers(dest="command", required=True)
    pack = subcommands.add_parser("pack", help="write checked SRAM load and measurement metadata")
    pack.add_argument("elf", type=Path)
    pack.add_argument("output", type=Path)
    diff = subcommands.add_parser("compare", help="compare XIP ELFs and optional SRAM trials")
    diff.add_argument("bare_xip", type=Path)
    diff.add_argument("rtos_xip", type=Path)
    for variant in ("bare", "rtos"):
        diff.add_argument(f"--{variant}-sram", type=Path)
        diff.add_argument(f"--{variant}-log", type=Path, help="OpenOCD log from the same SRAM ELF")
    args = parser.parse_args()
    try:
        if args.command == "pack":
            image = inspect_image(args.elf.read_bytes(), xip=False)
            with args.output.open("x", encoding="ascii") as output:
                output.write(render_trial(image))
            print(f"{image.identity}: {image.payload_bytes} bytes at {image.vector:#x} ({image.digest})")
        else:
            print(json.dumps(compare(args), indent=2))
    except (OSError, ValueError) as error:
        parser.error(str(error))


if __name__ == "__main__":
    main()
