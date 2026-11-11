#!/usr/bin/env python3
"""Every literal string passed to StanceI18n.t()/f() in the QML has an entry in
every quickshell/stance/i18n/*.json with the same %N placeholders, and each file
has a "_name" (exit 1 otherwise). Entries that look unused are only listed:
some keys are looked up through variables."""
import json, pathlib, re, sys

root = pathlib.Path(__file__).resolve().parent.parent / "quickshell"
call = re.compile(r'StanceI18n\.[tf]\(\s*"((?:[^"\\]|\\.)*)"')
used = {}
for f in sorted(root.rglob("*.qml")):
    if f.name == "StanceI18n.qml":
        continue
    for n, line in enumerate(f.read_text().splitlines(), 1):
        for m in call.finditer(line):
            used.setdefault(json.loads(f'"{m.group(1)}"'), f"{f.relative_to(root)}:{n}")

listed = set(re.findall(r'"([A-Za-z_]+)"', re.search(r"codes: \[(.*?)\]", (root / "stance" / "StanceI18n.qml").read_text()).group(1)))
files = sorted((root / "stance" / "i18n").glob("*.json"))
fail = False
for path in files:
    code = path.stem
    tr = json.loads(path.read_text())
    name = tr.pop("_name", None)
    if not isinstance(name, str) or not name:
        print(f"{code}: missing _name"); fail = True
    if not re.fullmatch(r"[a-z]{2,3}(_[A-Z]{2})?", code):
        print(f"{code}: bad language code"); fail = True
    if code not in listed:
        print(f"{code}: not listed in StanceI18n.codes"); fail = True
    missing = sorted(set(used) - set(tr))
    unused = sorted(set(tr) - set(used))
    bad = [k for k in used if k in tr and set(re.findall(r"%\d", k)) != set(re.findall(r"%\d", tr[k]))]
    for k in missing:
        print(f"{code}: missing: {k!r}  ({used[k]})")
    for k in bad:
        print(f"{code}: placeholder mismatch: {k!r} -> {tr[k]!r}")
    for k in unused:
        print(f"{code}: unused: {k!r}")
    print(f"{code}: {len(used)} strings, {len(tr)} translations, {len(missing)} missing, {len(unused)} unused, {len(bad)} bad placeholders")
    fail = fail or bool(missing or bad)
for c in sorted(listed - {p.stem for p in files}):
    print(f"{c}: listed but no file"); fail = True
sys.exit(1 if fail else 0)
