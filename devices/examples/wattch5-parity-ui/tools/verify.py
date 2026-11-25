"""Compare actual Rust RGB565 rows against independent approved V2 outputs."""
from pathlib import Path
import hashlib
import json
import sys

root = Path(sys.argv[1])
manifest = json.loads((root / "manifest.json").read_text())
assert manifest["cases"] == 1757
results = []
for index in range(manifest["cases"]):
    case = f"{index:04d}"
    expected = (root / f"{case}.reference.rgb565").read_bytes()
    actual = (root / f"{case}.actual.rgb565").read_bytes()
    if len(actual) != 32768 or expected != actual:
        raise ValueError("V2 reference mismatch: " + case)
    results.append({"id": case, "pixels": 16384, "sha256": hashlib.sha256(actual).hexdigest()})
report = {"states": len(results), "pixels": len(results) * 16384, "mismatches": 0, "results": results}
(root / "comparison.json").write_text(json.dumps(report, indent=2) + "\n")
print(f"PASS V2: {report['states']} states, {report['pixels']} pixels, zero differences")
