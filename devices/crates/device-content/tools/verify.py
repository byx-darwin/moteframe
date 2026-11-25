#!/usr/bin/env python3
"""Offline deterministic checks; no device access or repository mutations outside this crate."""
import hashlib
import json
from pathlib import Path
import subprocess
import sys
from datetime import datetime, timezone

ROOT = Path(__file__).resolve().parents[1]
LOGS = ROOT / "target" / "verification"
LOGS.mkdir(parents=True, exist_ok=True)
FEATURES = ["phone", "media", "gimbal", "find", "notifications", "wallet", "watchface", "companion-radio", "weather", "contacts", "memo"]
commands = [("fmt", ["cargo", "fmt", "--check"])]
for feature in ["default", *FEATURES, "all"]:
    selection = ["--no-default-features"]
    if feature == "all":
        selection += ["--all-features"]
    elif feature != "default":
        selection += ["--features", feature]
    commands += [
        (feature + "-host-test", ["cargo", "test", *selection]),
        (feature + "-host-clippy", ["cargo", "clippy", "--all-targets", *selection, "--", "-D", "warnings"]),
        (feature + "-arm-check", ["cargo", "check", "--target", "thumbv6m-none-eabi", *selection]),
    ]
commands.append(("all-arm-clippy", ["cargo", "clippy", "--lib", "--all-features", "--target", "thumbv6m-none-eabi", "--", "-D", "warnings"]))
results = []
for name, command in commands:
    process = subprocess.run(command, cwd=ROOT, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, check=False)
    log = LOGS / (name + ".log")
    log.write_text(process.stdout)
    results.append({"name": name, "command": command, "exit_code": process.returncode, "log": str(log.relative_to(ROOT)), "output_sha256": hashlib.sha256(process.stdout.encode()).hexdigest()})
    print(name, process.returncode, flush=True)
files = [ROOT / "Cargo.toml", ROOT / "Cargo.lock", ROOT / "DEVELOPMENT.md", *sorted((ROOT / "src").glob("*.rs")), *sorted((ROOT / "tests").glob("*.rs")), Path(__file__).resolve()]
report = {"crate": "device-content", "generated_utc": datetime.now(timezone.utc).isoformat(), "offline_only": True, "hardware_tested": False, "production_simulated_backend": False, "default_features": [], "features": FEATURES, "external_catalog_routes": 16, "all_feature_test_cases": 42, "passed": all(result["exit_code"] == 0 for result in results), "checks": results, "files": {str(path.relative_to(ROOT)): hashlib.sha256(path.read_bytes()).hexdigest() for path in files}}
(ROOT / "verification.json").write_text(json.dumps(report, indent=2) + "\n")
sys.exit(0 if report["passed"] else 1)
