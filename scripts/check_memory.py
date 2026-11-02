#!/usr/bin/env python3
"""Measure host/ARM layouts and fixed-workload object code without hardware."""
from pathlib import Path
import argparse
import io
import json
import os
import re
import struct
import subprocess
import tarfile
import tempfile

TARGET = "thumbv6m-none-eabi"
ROOT = Path(__file__).resolve().parents[1]
NAMES = {"MF_" + name for name in [
    "PIXEL_RGB565", "PIXEL_RGB888", "PIXEL_GRAY8", "PIXEL_BINARY",
    "RUN_RGB565", "RUN_RGB888", "RUN_GRAY8", "RUN_BINARY",
    "ROWS_RGB565", "ROWS_RGB888", "ROWS_GRAY8", "ROWS_BINARY",
    "STREAM", "TILE", "DIRTY", "DRIVER",
]}


def read_layout(ir, names):
    """Require exactly one compiler-emitted byte-array size per named layout."""
    result = {}
    for name in names:
        matches = re.findall(r"^@[^\s=]*" + re.escape(name) + r"[^\s=]*\s*=.*?constant \[(\d+) x i8\]", ir, re.M)
        if len(matches) != 1:
            raise ValueError("missing or duplicate compiler layout: " + name)
        result[name] = int(matches[0])
    return result


def read_object(data):
    """Read executable bytes and per-function frames from LLVM ARM ELF32 output."""
    if len(data) < 52 or data[:6] != b"\x7fELF\x01\x01":
        raise ValueError("expected little-endian ELF32 object")
    header = struct.unpack_from("<HHIIIIIHHHHHH", data, 16)
    _, machine, _, _, _, offset, _, _, _, _, stride, count, strings = header
    if machine != 40 or stride != 40 or not count or strings >= count or offset + count * stride > len(data):
        raise ValueError("invalid ARM section table")
    sections = [struct.unpack_from("<IIIIIIIIII", data, offset + i * stride) for i in range(count)]

    def content(section):
        start, size = section[4:6]
        if start + size > len(data):
            raise ValueError("truncated ELF section")
        return data[start:start + size]

    string_data = content(sections[strings])
    names = []
    for section in sections:
        start = section[0]
        end = string_data.find(b"\0", start)
        if start >= len(string_data) or end < 0:
            raise ValueError("invalid ELF section name")
        names.append(string_data[start:end].decode("ascii"))
    frames = {}
    for section, name in zip(sections, names):
        if name != ".stack_sizes":
            continue
        link = section[6]
        if not 0 < link < count:
            raise ValueError("stack-size section has no associated function")
        payload = content(section)
        position = 0
        while position < len(payload):
            position += 4  # Relocated function address, not an absolute address.
            value, shift = 0, 0
            while True:
                if position >= len(payload) or shift > 28:
                    raise ValueError("truncated or oversized stack-size entry")
                byte = payload[position]
                position += 1
                value |= (byte & 127) << shift
                if not byte & 128:
                    break
                shift += 7
            if names[link] in frames:
                raise ValueError("duplicate function frame")
            frames[names[link]] = value
    if not frames:
        raise ValueError("compiler object contains no stack-size evidence")
    return {"executable_section_bytes": sum(s[5] for s in sections if s[2] & 4),
            "function_frames": frames}


def run(arguments, cwd=ROOT, env=None):
    result = subprocess.run(arguments, cwd=cwd, capture_output=True, text=True, check=False, env=env)
    if result.returncode:
        raise RuntimeError("command failed: " + " ".join(arguments) + "\n" + result.stdout + result.stderr)
    return result.stdout


def build_libraries(root):
    output = run(["cargo", "build", "--workspace", "--lib", "--release", "--target", TARGET,
                  "--offline", "--locked", "--message-format=json"], cwd=root)
    libraries = {}
    for line in output.splitlines():
        record = json.loads(line)
        if record.get("reason") == "compiler-artifact":
            name = record["target"]["name"]
            if name in {"moteframe_core", "moteframe_ui", "moteframe_display"}:
                libraries[name] = next(path for path in record["filenames"] if path.endswith(".rlib"))
    if len(libraries) != 3:
        raise ValueError("missing ARM library artifacts")
    return libraries


def compile_probe(source, libraries, destination, rgb565_only):
    arguments = ["rustc", str(source), "--crate-name", "moteframe_memory", "--crate-type", "lib",
                 "--edition=2024", "--target", TARGET, "-Copt-level=s", "-Cpanic=abort",
                 "-Zemit-stack-sizes", "--check-cfg", "cfg(rgb565_only)",
                 "--emit", f"llvm-ir={destination}.ll,obj={destination}.o"]
    for name, path in libraries.items():
        arguments += ["--extern", name + "=" + path]
    directory = Path(next(iter(libraries.values()))).parent
    arguments += ["-L", "dependency=" + str(directory), "-L", "dependency=" + str(directory / "deps")]
    if rgb565_only:
        arguments += ["--cfg", "rgb565_only"]
    # Only the diagnostic rustc invocation unlocks frame metadata. Runtime crates
    # and Cargo builds remain stable Rust without this environment override.
    run(arguments, env={**os.environ, "RUSTC_BOOTSTRAP": "1"})
    return {"layout": read_layout(Path(str(destination) + ".ll").read_text(), NAMES),
            "object": read_object(Path(str(destination) + ".o").read_bytes())}


def baseline_tree(revision, destination):
    archive_bytes = subprocess.check_output(["git", "archive", revision], cwd=ROOT)
    destination.mkdir()
    with tarfile.open(fileobj=io.BytesIO(archive_bytes)) as archive:
        for member in archive.getmembers():
            path = destination / member.name
            if not path.resolve().is_relative_to(destination.resolve()):
                raise ValueError("escaping archive path")
            if member.isdir():
                path.mkdir(parents=True, exist_ok=True)
            elif member.isfile():
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(archive.extractfile(member).read())
            else:
                raise ValueError("unsupported archive entry")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline", help="optional reviewed 0.1 source revision")
    args = parser.parse_args()
    output = ROOT / "target/color-types/memory"
    output.mkdir(parents=True, exist_ok=True)
    source = ROOT / "crates/moteframe-ui/examples/memory.rs"
    host = json.loads(run(["cargo", "run", "-p", "moteframe-ui", "--example", "memory", "--offline", "--locked"]))
    libraries = build_libraries(ROOT)
    report = {"compiler": run(["rustc", "--version"]).strip(), "target": TARGET,
              "host": host,
              "rgb565": compile_probe(source, libraries, output / "rgb565", True),
              "all_colors": compile_probe(source, libraries, output / "native", False),
              "probe_flags": "opt-level=s, panic=abort, no final link/LTO; diagnostic rustc only: RUSTC_BOOTSTRAP=1 -Zemit-stack-sizes",
              "limits": "Object executable sections and per-function frames only; not linked firmware, a complete call-chain bound, interrupts, or a device watermark."}
    if args.baseline:
        # Use a new output directory for each invocation, preserving earlier evidence.
        destination = Path(tempfile.mkdtemp(prefix="baseline-", dir=output)) / "source"
        baseline_tree(args.baseline, destination)
        old_source = output / "baseline-probe.rs"
        text = source.read_text().replace("Tile<'a, Rgb565>", "Tile<'a>")
        text = text.replace("let mut scratch = [Rgb565::BLACK; 16];", "let mut scratch = [moteframe_core::Rgb565(0); 16];")
        old_source.write_text(text)
        report["baseline_revision"] = args.baseline
        report["baseline"] = compile_probe(old_source, build_libraries(destination), output / "baseline", True)
    for name in NAMES:
        if host[name] <= 0 or report["rgb565"]["layout"][name] <= 0:
            raise ValueError("invalid zero layout: " + name)
    for probe in ["rgb565", "all_colors"] + (["baseline"] if args.baseline else []):
        frames = report[probe]["object"]["function_frames"]
        if not any("probe_rgb565" in name for name in frames) or not any("probe_repaint" in name for name in frames):
            raise ValueError("missing required probe frames")
    name = "comparison.json" if args.baseline else "report.json"
    (output / name).write_text(json.dumps(report, indent=2) + "\n")
    print("Host/ARM color layouts and compiler object/frame evidence: target/color-types/memory/" + name)


if __name__ == "__main__":
    main()
