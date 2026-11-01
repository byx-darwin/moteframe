#!/usr/bin/env python3
"""Audit Cargo package file lists without creating or publishing packages."""
from pathlib import Path, PurePosixPath
import subprocess
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))
from check_boundary import RUST_FILES


def validate_package(package, files):
    """Require the license and attribution notice; reject unreviewed files."""
    if package not in RUST_FILES:
        return ["package is outside portable scope"]
    required = RUST_FILES[package] | {"Cargo.toml", "README.md", "LICENSE", "NOTICE"}
    permitted = required | {"Cargo.lock", "Cargo.toml.orig", ".cargo_vcs_info.json"}
    errors = []
    for name in files:
        path = PurePosixPath(name)
        if path.is_absolute() or ".." in path.parts or name not in permitted:
            errors.append("unexpected package file: " + name)
    for name in sorted(required - set(files)):
        errors.append("missing required package file: " + name)
    return errors


def main():
    root = Path(__file__).resolve().parents[1]
    for package in sorted(RUST_FILES):
        result = subprocess.run(
            ["cargo", "package", "-p", package, "--list", "--offline", "--locked", "--allow-dirty"],
            cwd=root, text=True, capture_output=True, check=False,
        )
        if result.returncode:
            print(result.stderr, file=sys.stderr)
            return result.returncode
        files = result.stdout.splitlines()
        errors = validate_package(package, files)
        if errors:
            print(package + ": " + "; ".join(errors), file=sys.stderr)
            return 1
        print(f"{package}: {len(files)} reviewed package files; no publication performed.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
