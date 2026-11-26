#!/usr/bin/env python3
"""Generate offline OpenOCD inputs for the FR8029D two-sector boot trial."""
import argparse
import json
from hashlib import sha256
from pathlib import Path
import struct

from pack_probe import APP_OFFSET, SECTOR_BYTES, build_package


def load_page_helper(elf):
    """Read the relocation-free code and verify its fixed breakpoint offsets."""
    if len(elf) < 52 or elf[:7] != b"\x7fELF\x01\x01\x01":
        raise ValueError("Expected a little-endian ARM32 page writer object")
    header = struct.unpack_from("<16sHHIIIIIHHHHHH", elf)
    offset, size, count, name_index = header[6], header[11], header[12], header[13]
    if (header[1:4] != (1, 40, 1) or size != 40 or offset < 52
            or offset + size * count > len(elf) or not 0 < name_index < count):
        raise ValueError("Invalid page writer section headers")
    sections = [struct.unpack_from("<10I", elf, offset + size * i) for i in range(count)]
    strings = sections[name_index]
    names = elf[strings[4]:strings[4] + strings[5]]
    if strings[1] != 3 or len(names) != strings[5] or any(s[1] in (4, 9) for s in sections):
        raise ValueError("The page writer must have no relocations")
    text = [s for s in sections if names[s[0]:].split(b"\0", 1)[0] == b".text"]
    if len(text) != 1 or text[0][1:4] != (1, 6, 0):
        raise ValueError("Expected one executable page writer code section")
    data = elf[text[0][4]:text[0][4] + text[0][5]]
    if (len(data) != 60 or data[0x32:0x34] != b"\x29\xbe"
            or data[0x38:0x3a] != b"\x2a\xbe"):
        raise ValueError("Page helper does not match the tested breakpoint layout")
    return data


def tcl_words(data):
    """Serialize a little-endian byte sequence as a Tcl list of words."""
    return "{" + " ".join(hex(word) for (word,) in struct.iter_unpack("<I", data)) + "}"


def prepare(original_path, elf_path, package, helper_path):
    """Recompute the expected overlay and independently verify the saved sectors."""
    original = original_path.read_bytes()
    elf = elf_path.read_bytes()
    candidate, patches = build_package(original, elf)
    manifest = json.loads((package / "manifest.json").read_text(encoding="utf-8"))
    hashes = {
        "original_sha256": sha256(original).hexdigest(),
        "candidate_overlay_sha256": sha256(candidate).hexdigest(),
        "probe_elf_sha256": sha256(elf).hexdigest(),
    }
    if any(manifest.get(key) != value for key, value in hashes.items()):
        raise ValueError("Package manifest does not match the backup and ELF")
    for patch in patches:
        for role, content in (("probe", patch.replacement), ("restore", patch.original)):
            file = package / f"sector-{patch.offset:05x}-{role}.bin"
            if file.read_bytes() != content:
                raise ValueError(f"Package sector differs: {file.name}")

    lines = []
    for name, image in (("original", original), ("candidate", candidate)):
        app = tcl_words(image[APP_OFFSET:APP_OFFSET + SECTOR_BYTES])
        header = tcl_words(image[:SECTOR_BYTES])
        lines.append(f"set {name} [dict create 0x2000 {app} 0x0 {header}]")
    # Quote the retained backup path as a literal Tcl string.
    path = str(original_path.resolve())
    quoted = "".join(("\\" + ch) if ch in '\\"$[]' else ch for ch in path)
    lines.append(f'dict set original backup_file "{quoted}"')
    lines.append("dict set original backup_digest " + tcl_words(sha256(original).digest()))
    lines.append("set ota " + tcl_words(original[SECTOR_BYTES:APP_OFFSET]))
    lines.append("set helper " + tcl_words(load_page_helper(helper_path.read_bytes())))
    return "\n".join(lines) + "\n"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("original", "elf", "package", "helper", "output"):
        parser.add_argument(f"--{name}", required=True, type=Path)
    args = parser.parse_args()
    try:
        script = prepare(args.original, args.elf, args.package, args.helper)
        with args.output.open("x", encoding="ascii") as stream:
            stream.write(script)
    except (OSError, ValueError) as error:
        parser.error(str(error))


if __name__ == "__main__":
    main()
