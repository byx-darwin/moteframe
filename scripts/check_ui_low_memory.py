#!/usr/bin/env python3
"""Test and ARM-build the standalone application in both real RAM profiles."""
from pathlib import Path
import json
import os
import re
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
APP = ROOT / "examples/ui-low-memory"
MANIFEST = APP / "Cargo.toml"
OUT = ROOT / "target/ui-low-memory"


def run(arguments, log):
    """Use the checked-in standalone lock, offline, in a stable target folder."""
    environment = dict(os.environ, CARGO_TARGET_DIR=str(OUT / "build"))
    result = subprocess.run(
        ["cargo", arguments[0], "--manifest-path", str(MANIFEST), "--offline", "--locked",
         *arguments[1:]],
        cwd=ROOT, env=environment, capture_output=True, text=True, check=False,
    )
    (OUT / log).write_text(result.stdout + result.stderr)
    if result.returncode:
        raise RuntimeError(f"Cargo failed ({log}):\n{result.stdout}{result.stderr}")
    return result.stdout


def validate_metadata(metadata, low_memory):
    """Reject accidental workspace membership, publication or dependency drift."""
    package = next((p for p in metadata["packages"]
                    if Path(p["manifest_path"]) == MANIFEST), None)
    if package is None or package["publish"] != []:
        raise RuntimeError("the standalone example must exist and set publish=false")
    if (Path(metadata["workspace_root"]) != APP
            or metadata["workspace_members"] != [package["id"]]):
        raise RuntimeError("the example must own a standalone [workspace]")
    if package["features"] != {"default": [], "ui-low-memory": []}:
        raise RuntimeError("the app must own only an empty-default RAM feature")
    dependencies = {d["name"]: d for d in package["dependencies"]}
    expected = {"moteframe-core": [], "moteframe-ui": ["refresh"]}
    if set(dependencies) != set(expected):
        raise RuntimeError("unexpected application dependencies")
    # Metadata normalizes paths. Inspect original literals too: this sample must
    # remain copyable with repo-relative dependencies, without machine paths.
    paths = re.findall(r'\bpath\s*=\s*"([^"\n]+)"', MANIFEST.read_text())
    if len(paths) != 2 or set(paths) != {
        "../../crates/moteframe-core", "../../crates/moteframe-ui"
    }:
        raise RuntimeError("application dependencies must use reviewed relative paths")
    for name, features in expected.items():
        dependency = dependencies[name]
        if (Path(dependency["path"]) != ROOT / "crates" / name
                or dependency["uses_default_features"]
                or dependency["features"] != features
                or dependency["kind"] is not None):
            raise RuntimeError(f"invalid application dependency: {name}")
    nodes = {n["id"]: n for n in metadata["resolve"]["nodes"]}
    wanted = ["default", "ui-low-memory"] if low_memory else ["default"]
    if sorted(nodes[package["id"]]["features"]) != wanted:
        raise RuntimeError("the requested application profile was not resolved")
    ui = next(p for p in metadata["packages"] if p["name"] == "moteframe-ui")
    if nodes[ui["id"]]["features"] != ["refresh"]:
        raise RuntimeError("the standalone example unexpectedly enabled other UI features")


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    observations = []
    for low_memory in (False, True):
        name = "low-memory" if low_memory else "default"
        features = ["--features", "ui-low-memory"] if low_memory else []
        metadata = json.loads(run(["metadata", "--format-version", "1", *features],
                                  f"{name}-metadata.log"))
        validate_metadata(metadata, low_memory)
        output = run(["test", *features, "--", "--nocapture"], f"{name}-tests.log")
        matches = re.findall(r"image_hashes=(\[[0-9, ]+\])", output)
        if len(matches) != 1:
            raise RuntimeError(f"{name}: missing independent-image test evidence")
        hashes = json.loads(matches[0])
        if len(hashes) != 4:
            raise RuntimeError(f"{name}: expected four scene inputs")
        run(["build", "--release", "--target", "thumbv6m-none-eabi", *features],
            f"{name}-arm-build.log")
        observations.append({"profile": name, "pixels": 8 if low_memory else 32,
                             "image_hashes": hashes, "host_tests": "passed",
                             "arm_build": "passed"})
        print(f"{name}: host behavior tests and Cortex-M0 build passed")
    if observations[0]["image_hashes"] != observations[1]["image_hashes"]:
        raise RuntimeError("RAM profiles produced different final images")
    (OUT / "report.json").write_text(json.dumps(observations, indent=2) + "\n")
    print("RAM switch passed: identical final images; actual 32/8-pixel arrays and runs.")
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (RuntimeError, KeyError, ValueError, OSError) as error:
        print(str(error), file=sys.stderr)
        sys.exit(1)
