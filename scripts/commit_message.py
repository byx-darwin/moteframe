#!/usr/bin/env python3
"""Validate Conventional Commit subjects without reading or printing the body."""
from pathlib import Path
import re
import sys


def valid_subject(subject):
    return re.fullmatch(
        r"(?:feat|fix|docs|refactor|test|chore|build|ci|perf|revert)"
        r"(?:\([A-Za-z0-9_./-]+\))?!?: [^\s].*", subject
    ) is not None


if __name__ == "__main__":
    lines = Path(sys.argv[1]).read_text().splitlines()
    if not lines or not valid_subject(lines[0]):
        sys.exit("Commit subject must use Conventional Commits, e.g. feat(display): stream RGB565")
