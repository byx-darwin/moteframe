#!/usr/bin/env python3
"""Offline feature/transport integration matrix, writing only this crate's evidence."""
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import re
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
LOGS = ROOT / "target" / "verification"
LOGS.mkdir(parents=True, exist_ok=True)
MODULES = ["phone", "media", "find", "notifications", "companion-radio", "gimbal"]
selections = [("default", ["--no-default-features"])]
selections += [(feature, ["--no-default-features", "--features", feature]) for feature in [*MODULES, "event-pump"]]
selections += [(feature + "-pump", ["--no-default-features", "--features", feature + ",event-pump"]) for feature in MODULES]
selections.append(("all", ["--all-features"]))
commands = [("fmt", ["cargo", "fmt", "--check"])]
for name, selection in selections:
    commands += [
        (name + "-host-test", ["cargo", "test", *selection]),
        (name + "-host-clippy", ["cargo", "clippy", "--all-targets", *selection, "--", "-D", "warnings"]),
        (name + "-arm-check", ["cargo", "check", "--target", "thumbv6m-none-eabi", *selection]),
    ]
commands.append(("all-arm-clippy", ["cargo", "clippy", "--all-features", "--lib", "--target", "thumbv6m-none-eabi", "--", "-D", "warnings"]))
results = []
all_tests = 0
for name, command in commands:
    process = subprocess.run(command, cwd=ROOT, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, check=False)
    (LOGS / (name + ".log")).write_text(process.stdout)
    results.append({"name": name, "command": command, "exit_code": process.returncode, "log": "target/verification/" + name + ".log", "output_sha256": hashlib.sha256(process.stdout.encode()).hexdigest()})
    if name == "all-host-test":
        all_tests = sum(map(int, re.findall(r"test result: ok\. (\d+) passed", process.stdout)))
    print(name, process.returncode, flush=True)
files = [ROOT / "Cargo.toml", ROOT / "Cargo.lock", ROOT / "DEVELOPMENT.md", *sorted((ROOT / "src").rglob("*.rs")), *sorted((ROOT / "tests").rglob("*.rs")), Path(__file__).resolve()]
dependencies = {}
for dependency in ["device-content", "device-protocol", "device-services", "device-connection"]:
    directory = ROOT.parent / dependency
    selected = [directory / "Cargo.toml", *sorted((directory / "src").rglob("*.rs"))]
    dependencies[dependency] = {str(path.relative_to(directory)): hashlib.sha256(path.read_bytes()).hexdigest() for path in selected}
report = {"crate": "device-content-service", "generated_utc": datetime.now(timezone.utc).isoformat(), "offline_only": True, "hardware_tested": False, "production_simulated_backend": False, "default_features": [], "module_features": MODULES, "optional_transport_feature": "event-pump", "business_module_version": "1.0", "wire_version": "1.2", "find_e80_candidate_bytes": 8, "control_events_have_invented_crc": False, "all_feature_test_cases": all_tests, "passed": all(result["exit_code"] == 0 for result in results), "checks": results, "files": {str(path.relative_to(ROOT)): hashlib.sha256(path.read_bytes()).hexdigest() for path in files}, "dependency_source_snapshot": dependencies}
(ROOT / "verification.json").write_text(json.dumps(report, indent=2) + "\n")
sys.exit(0 if report["passed"] else 1)
