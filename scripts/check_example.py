#!/usr/bin/env python3
"""Validate the real procedural renderer output without an image dependency."""
from pathlib import Path
import argparse
import sys

HEADER = b"P6\n64 40\n255\n"
PIXEL_BYTES = 7680


def validate(data):
    """Require exact geometry, full RGB bytes and all primitive colors."""
    if not data.startswith(HEADER):
        return ["expected binary RGB PPM, 64x40, maximum channel 255"]
    pixels = data[len(HEADER):]
    if len(pixels) != PIXEL_BYTES:
        return ["pixel payload must contain exactly 2560 RGB pixels"]
    actual = {pixels[n:n + 3] for n in range(0, len(pixels), 3)}
    colors = {b"\x00\x00\x00", b"\xff\x00\x00", b"\x00\xff\x00", b"\x00\x00\xff", b"\xff\xff\xff"}
    if not colors.issubset(actual):
        return ["rendered frame is missing a procedural primitive color"]
    return []


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("image", type=Path)
    args = parser.parse_args()
    with args.image.open("rb") as source:
        data = source.read(len(HEADER) + PIXEL_BYTES + 1)
    errors = validate(data)
    if errors:
        print("; ".join(errors), file=sys.stderr)
        return 1
    print("Host render passed: 64x40 complete RGB pixels, black/red/green/blue/white present.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
