#!/usr/bin/env bash
# Converter test: tests/theme-convert.sh [path/to/stance]  (stance optional: also runs `theme check`)
set -euo pipefail
cd "$(dirname "$0")/.."
conv=contrib/stance-theme-from-pywal
stance=${1:-}
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

python3 "$conv" tests/fixtures/pywal-colors.json > "$tmp/wal.json"
python3 "$conv" --matugen tests/fixtures/matugen.json --scheme light --id mat-l > "$tmp/mat.json"
python3 - "$tmp" <<'P'
import json, sys
for f, dark in (("wal", True), ("mat", False)):
    t = json.load(open(f"{sys.argv[1]}/{f}.json"))
    assert t["stance_theme"] == 1 and len(t["colors"]) == 12, f
    assert t["dark"] is dark, f
P
if python3 "$conv" tests/fixtures/pywal-lowcontrast.json >/dev/null 2>"$tmp/err"; then
  echo "low-contrast palette was not refused" >&2
  exit 1
fi
grep -q "low contrast" "$tmp/err"
python3 "$conv" --force tests/fixtures/pywal-lowcontrast.json >/dev/null
if [ -n "$stance" ]; then
  "$stance" theme check "$tmp/wal.json"
  "$stance" theme check "$tmp/mat.json"
fi
echo "theme-convert: ok"
