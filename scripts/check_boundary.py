#!/usr/bin/env python3
"""Check the initial portable source allowlist and standalone Cargo graph."""
from pathlib import Path
import argparse
import json
import os
import re
import subprocess
import sys

PACKAGES = {"moteframe-core", "moteframe-ui", "moteframe-display", "moteframe-i2c"}
UPSTREAM = {"embedded-hal", "embedded-graphics"}
RESOLVED_UPSTREAM = UPSTREAM | {
    "autocfg", "az", "byteorder", "embedded-graphics-core", "float-cmp", "micromath", "num-traits",
}
REGISTRY = "registry+https://github.com/rust-lang/crates.io-index"
RUST_FILES = {
    "moteframe-core": {"src/lib.rs", "src/read.rs", "src/spi.rs", "tests/shared_spi.rs"},
    "moteframe-ui": {
        "src/lib.rs", "src/geometry.rs", "src/layout.rs", "src/refresh.rs", "src/stream.rs",
        "tests/damage.rs", "tests/refresh.rs", "tests/cooperative.rs", "tests/foundation.rs",
        "tests/dcs.rs", "examples/offline.rs",
    },
    "moteframe-display": {"src/lib.rs", "src/spi.rs", "src/shared.rs", "tests/shared.rs", "tests/spi.rs", "tests/stream.rs"},
    "moteframe-i2c": {"src/lib.rs", "tests/read.rs"},
}
ROOT_FILES = {
    ".gitignore", ".pre-commit-config.yaml", ".typos.toml", ".gitleaks.toml", "CLAUDE.md", "Cargo.toml",
    "Cargo.lock", "Makefile", "rust-toolchain.toml", "deny.toml", "README.md",
    "CONTRIBUTING.md", "LICENSE", "NOTICE", "NOTICE.md",
    ".github/workflows/check.yml",
    "docs/progress.md", "docs/verification.md",
    "docs/plans/2026-11-02-foundation-extraction.md", "specs/foundation-extraction.md",
    "docs/plans/2026-11-02-publication.md",
}
SCRIPTS = {
    "scripts/check_boundary.py", "scripts/commit_message.py", "scripts/check_example.py",
    "scripts/check_packages.py", "scripts/tests/test_boundary.py",
    "scripts/tests/test_commit_message.py", "scripts/tests/test_example.py",
    "scripts/tests/test_packages.py",
}
BINARY_SUFFIXES = {".bin", ".lib", ".a", ".elf", ".hex", ".pdf", ".doc", ".docx", ".png", ".jpg", ".zip", ".7z", ".rar", ".pem", ".key"}


def contained(path, root):
    """Resolve paths before testing containment, including symlink escapes."""
    try:
        path.resolve().relative_to(root.resolve())
        return True
    except ValueError:
        return False


def allowed(relative):
    if relative in ROOT_FILES or relative in SCRIPTS:
        return True
    path = Path(relative)
    parts = path.parts
    if len(parts) < 3 or parts[0] != "crates" or parts[1] not in RUST_FILES:
        return False
    tail = Path(*parts[2:]).as_posix()
    return tail in RUST_FILES[parts[1]] or tail in {"Cargo.toml", "README.md", "LICENSE", "NOTICE"}


def validate_files(root):
    """Return violations without reading outside root or following symlinks."""
    errors = []
    for directory, directories, files in os.walk(root, followlinks=False):
        base = Path(directory)
        for name in list(directories):
            path = base / name
            if path.is_symlink():
                errors.append("symlink: " + path.relative_to(root).as_posix())
                directories.remove(name)
            elif name == "__pycache__" or (base == root and name in {".git", "target", ".superpowers"}):
                directories.remove(name)
        for name in files:
            path = base / name
            relative = path.relative_to(root).as_posix()
            if path.is_symlink():
                errors.append("symlink: " + relative)
                continue
            if name == ".DS_Store" or path.suffix == ".pyc":
                continue
            if path.suffix.lower() in BINARY_SUFFIXES:
                errors.append("binary/resource: " + relative)
            elif not allowed(relative):
                errors.append("outside portable scope: " + relative)
            if path.suffix == ".rs":
                text = path.read_text()
                if re.search(r"syd881|sydtek|wattch|jlinkarm", text, re.I):
                    errors.append("private/platform identifier: " + relative)
                if re.search(r"include_(?:bytes|str)!", text):
                    errors.append("embedded resource: " + relative)
                if re.search(r"\bunsafe\s+(?:\{|impl\b|extern\b|fn\b)|\basm!|\bglobal_asm!", text):
                    errors.append("unsafe/platform code: " + relative)
    return errors


def validate_tracked(paths):
    """Forced Git additions must not bypass ignored-artifact exclusions."""
    return ["unreviewed tracked file: " + path for path in paths if not allowed(path)]


def validate_metadata(metadata, root):
    """Validate full resolution as well as declared dependencies and source targets."""
    errors = []
    if Path(metadata["workspace_root"]).resolve() != root.resolve():
        errors.append("workspace root is not standalone")
    packages = metadata["packages"]
    members = set(metadata["workspace_members"])
    selected = [p for p in packages if p["id"] in members]
    if {p["name"] for p in selected} != PACKAGES:
        errors.append("workspace members exceed or omit portable scope")
    ids = {p["id"] for p in packages}
    graph = metadata.get("resolve")
    if graph is None:
        errors.append("full resolved graph is required")
    else:
        nodes = graph["nodes"]
        if {node["id"] for node in nodes} != ids or not members <= ids:
            errors.append("resolved graph does not cover all packages and workspace members")
        if any(dep not in ids for node in nodes for dep in node["dependencies"]):
            errors.append("resolved graph references unknown packages")
    for package in packages:
        if package["id"] not in members:
            if package["name"] not in RESOLVED_UPSTREAM or package.get("source") != REGISTRY:
                errors.append("unapproved resolved dependency or external path patch: " + package["name"])
    for package in selected:
        name = package["name"]
        manifest = Path(package["manifest_path"])
        if not contained(manifest, root):
            errors.append("manifest outside standalone tree: " + name)
        elif manifest.resolve() != (root / "crates" / name / "Cargo.toml").resolve():
            errors.append("manifest is not at the reviewed crate path: " + name)
        if package.get("source") is not None:
            errors.append("workspace package is not a local source: " + name)
        for target in package.get("targets", []):
            source = Path(target["src_path"])
            reviewed = {root / "crates" / name / relative for relative in RUST_FILES.get(name, set())}
            if not contained(source, root) or source not in reviewed or source.resolve() not in {p.resolve() for p in reviewed}:
                errors.append("unreviewed target source: " + name)
        if package.get("license") != "Apache-2.0":
            errors.append("unexpected first-party license: " + name)
        for dependency in package["dependencies"]:
            dep = dependency["name"]
            path = dependency.get("path")
            if path is not None:
                expected = root / "crates" / dep
                if dep not in PACKAGES or not contained(Path(path), root) or Path(path).resolve() != expected.resolve():
                    errors.append("escaping dependency or unexpected internal path: " + dep)
            elif dep not in UPSTREAM or dependency.get("source") != REGISTRY:
                errors.append("unapproved external dependency: " + dep)
    return errors


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    args = parser.parse_args()
    root = args.root.resolve()
    errors = validate_files(root)
    tracked = subprocess.run(["git", "ls-files", "--cached", "-z"], cwd=root,
                             capture_output=True, text=True, check=False)
    if tracked.returncode:
        errors.append("Git tracked-file check failed: " + tracked.stderr.strip())
    else:
        errors.extend(validate_tracked([path for path in tracked.stdout.split("\0") if path]))
    result = subprocess.run(
        ["cargo", "metadata", "--offline", "--locked", "--format-version", "1"],
        cwd=root, capture_output=True, text=True, check=False,
    )
    if result.returncode:
        errors.append("Cargo metadata failed: " + result.stderr.strip())
    else:
        errors.extend(validate_metadata(json.loads(result.stdout), root))
    if errors:
        for error in errors:
            print(error, file=sys.stderr)
        return 1
    print("Portable source allowlist and standalone dependency boundary passed.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
