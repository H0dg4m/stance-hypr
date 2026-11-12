#!/usr/bin/env bash
# Every Text/TextEdit sets textFormat: the default (AutoText) renders a
# device-chosen name like `<img src=…>` as HTML. Fails listing offenders.
set -euo pipefail
cd "$(dirname "$0")/.."
python3 - <<'PY'
import pathlib, re, sys
bad = []
for f in sorted(pathlib.Path("quickshell").rglob("*.qml")):
    lines = f.read_text().split("\n")
    for i, l in enumerate(lines):
        if not re.match(r"^\s*(Text|TextEdit)\s*\{\s*$", l):
            continue
        depth, ok = 0, False
        for l2 in lines[i:]:
            depth += l2.count("{") - l2.count("}")
            if depth == 1 and re.match(r"^\s*textFormat\s*:", l2):
                ok = True
            if depth <= 0:
                break
        if not ok:
            bad.append(f"{f}:{i + 1}")
for b in bad:
    print("no textFormat:", b)
sys.exit(1 if bad else 0)
PY
echo "qml-plaintext: ok"
